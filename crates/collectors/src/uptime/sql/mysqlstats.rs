//! Santé interne d'un serveur MySQL ou MariaDB, lue sur la connexion de la sonde.
//!
//! Même esprit que [`super::pgstats`] : la sonde dit « la base répond », ce module
//! dit *comment elle va* : connexions contre `max_connections`, requêtes par
//! seconde (compteur `Questions`), requêtes lentes, taux de réussite du tampon
//! InnoDB, réplication et retard, taille des schémas, requêtes qui traînent.
//!
//! # Règles de conduite
//!
//! * **Aucun échec n'est une panne.** Chaque lecture est indépendante : un compte
//!   sans `PROCESS` ou `REPLICATION CLIENT` voit moins de choses, la lecture est
//!   alors omise, jamais comptée contre la sonde.
//! * **Même connexion, même budget** (voir [`collect`]).
//! * **Lecture seule** : `SHOW` et `SELECT` sur `information_schema`.
//! * **Cardinalité bornée** : dix schémas, huit canaux de réplication.
//!
//! Les noms sont ceux de `mysql_*`, sans le préfixe `probe_`.

use std::collections::HashMap;
use std::time::Duration;

use dumbmonit_proto::Sample;
use sqlx::Row;
use sqlx::mysql::{MySqlConnection, MySqlRow};
use tracing::debug;

use crate::rest::{counter, flag, gauge};
use crate::uptime::session::Deadline;

const MAX_SCHEMAS: usize = 10;
const MAX_CHANNELS: usize = 8;

const STATUS: &str = "SHOW GLOBAL STATUS WHERE Variable_name IN ('Uptime', 'Threads_connected', \
    'Threads_running', 'Questions', 'Slow_queries', 'Aborted_connects', 'Aborted_clients', \
    'Max_used_connections', 'Innodb_buffer_pool_read_requests', 'Innodb_buffer_pool_reads')";

const VARIABLES: &str = "SHOW GLOBAL VARIABLES WHERE Variable_name IN ('version', \
    'version_comment', 'max_connections', 'read_only')";

// `SHOW REPLICA STATUS` depuis MySQL 8.0.22 et MariaDB 10.5.1 ; l'ancienne forme
// reste le repli des versions antérieures.
const REPLICA: &str = "SHOW REPLICA STATUS";
const REPLICA_LEGACY: &str = "SHOW SLAVE STATUS";

// Tout est converti en texte : le type exact (DECIMAL, BIGINT) varie selon la
// version et le moteur, et le texte se lit partout.
const SCHEMAS: &str = "SELECT CAST(table_schema AS CHAR), \
    CAST(COALESCE(SUM(data_length + index_length), 0) AS CHAR) \
    FROM information_schema.tables \
    WHERE table_schema NOT IN ('information_schema', 'performance_schema', 'sys', 'mysql') \
    GROUP BY table_schema ORDER BY SUM(data_length + index_length) DESC LIMIT 10";

const LONG_QUERIES: &str = "SELECT \
    CAST(COALESCE(SUM(CASE WHEN time > 60 THEN 1 ELSE 0 END), 0) AS CHAR), \
    CAST(COALESCE(MAX(time), 0) AS CHAR) \
    FROM information_schema.processlist \
    WHERE command NOT IN ('Sleep', 'Daemon', 'Binlog Dump', 'Binlog Dump GTID') \
    AND user NOT IN ('system user', 'event_scheduler')";

/// Une ligne de `SHOW REPLICA STATUS` (une par canal).
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub name: String,
    pub io_running: bool,
    pub sql_running: bool,
    /// `NULL` quand la réplication est rompue : aucune valeur n'est publiée.
    pub lag_seconds: Option<f64>,
}

// ------------------------------------------------------------- traduction

fn number(map: &HashMap<String, String>, key: &str) -> Option<f64> {
    map.get(key).and_then(|value| value.trim().parse::<f64>().ok())
}

/// Valeur d'un état « Yes » / « Connecting » / « No » de la réplication.
pub fn is_yes(raw: &str) -> bool {
    raw.trim().eq_ignore_ascii_case("yes")
}

/// Booléen d'une variable : `ON`, `1`, `true`.
fn is_on(raw: &str) -> bool {
    matches!(raw.trim().to_ascii_lowercase().as_str(), "on" | "1" | "true")
}

/// Traduit `SHOW GLOBAL STATUS` et `SHOW GLOBAL VARIABLES`.
pub fn server_samples(
    status: &HashMap<String, String>,
    variables: &HashMap<String, String>,
    ts_ms: i64,
) -> Vec<Sample> {
    let mut out = Vec::new();
    if let Some(v) = number(status, "Uptime") {
        out.push(gauge("mysql_uptime_seconds", v, ts_ms));
    }
    if let Some(version) = variables.get("version").filter(|v| !v.is_empty()) {
        let mut info = gauge("mysql_version_info", 1.0, ts_ms).with_label("version", version);
        if let Some(comment) = variables.get("version_comment").filter(|c| !c.is_empty()) {
            info = info.with_label("flavor", comment);
        }
        out.push(info);
    }
    if let Some(read_only) = variables.get("read_only") {
        out.push(flag("mysql_read_only", is_on(read_only), ts_ms));
    }
    let connected = number(status, "Threads_connected");
    let max = number(variables, "max_connections");
    if let Some(v) = connected {
        out.push(gauge("mysql_connections", v, ts_ms));
    }
    if let Some(v) = max {
        out.push(gauge("mysql_max_connections", v, ts_ms));
    }
    if let (Some(used), Some(max)) = (connected, max)
        && max > 0.0
    {
        out.push(gauge("mysql_connections_used_percent", (used / max * 100.0).min(100.0), ts_ms));
    }
    if let Some(v) = number(status, "Threads_running") {
        out.push(gauge("mysql_threads_running", v, ts_ms));
    }
    if let Some(v) = number(status, "Max_used_connections") {
        out.push(gauge("mysql_max_used_connections", v, ts_ms));
    }
    // Compteurs : la courbe des requêtes par seconde est leur dérivée.
    for (key, metric) in [
        ("Questions", "mysql_queries_total"),
        ("Slow_queries", "mysql_slow_queries_total"),
        ("Aborted_connects", "mysql_aborted_connects_total"),
        ("Aborted_clients", "mysql_aborted_clients_total"),
        ("Innodb_buffer_pool_read_requests", "mysql_buffer_pool_read_requests_total"),
        ("Innodb_buffer_pool_reads", "mysql_buffer_pool_disk_reads_total"),
    ] {
        if let Some(v) = number(status, key) {
            out.push(counter(metric, v, ts_ms));
        }
    }
    if let Some(percent) = buffer_pool_hit_percent(status) {
        out.push(gauge("mysql_buffer_pool_hit_percent", percent, ts_ms));
    }
    out
}

/// Part des lectures servies par le tampon InnoDB, depuis le démarrage. `None`
/// tant que le serveur n'a rien lu : un taux sans lecture ne veut rien dire.
pub fn buffer_pool_hit_percent(status: &HashMap<String, String>) -> Option<f64> {
    let requests = number(status, "Innodb_buffer_pool_read_requests")?;
    let reads = number(status, "Innodb_buffer_pool_reads")?;
    (requests > 0.0).then(|| ((1.0 - reads / requests) * 100.0).clamp(0.0, 100.0))
}

pub fn replication_samples(channels: &[Channel], ts_ms: i64) -> Vec<Sample> {
    let mut out = vec![gauge("mysql_replicas_configured", channels.len() as f64, ts_ms)];
    for channel in channels.iter().take(MAX_CHANNELS) {
        let name = channel.name.as_str();
        let label = |s: Sample| s.with_label("channel", name);
        out.push(label(flag("mysql_replica_io_running", channel.io_running, ts_ms)));
        out.push(label(flag("mysql_replica_sql_running", channel.sql_running, ts_ms)));
        out.push(label(flag(
            "mysql_replica_running",
            channel.io_running && channel.sql_running,
            ts_ms,
        )));
        if let Some(lag) = channel.lag_seconds {
            out.push(label(gauge("mysql_replica_lag_seconds", lag.max(0.0), ts_ms)));
        }
    }
    out
}

pub fn schema_samples(schemas: &[(String, f64)], ts_ms: i64) -> Vec<Sample> {
    schemas
        .iter()
        .take(MAX_SCHEMAS)
        .map(|(name, bytes)| {
            gauge("mysql_database_size_bytes", *bytes, ts_ms).with_label("database", name)
        })
        .collect()
}

pub fn long_query_samples(count: f64, longest_seconds: f64, ts_ms: i64) -> Vec<Sample> {
    vec![
        gauge("mysql_long_running_queries", count, ts_ms),
        gauge("mysql_longest_query_seconds", longest_seconds, ts_ms),
    ]
}

// ------------------------------------------------------------- lecture

/// Une cellule lue comme texte, quel que soit le type que le serveur annonce
/// (selon la version, `SHOW` rend du texte, du binaire ou des entiers).
fn cell(row: &MySqlRow, key: impl sqlx::ColumnIndex<MySqlRow> + Copy) -> Option<String> {
    if let Ok(value) = row.try_get::<Option<String>, _>(key) {
        return value;
    }
    if let Ok(value) = row.try_get::<Option<Vec<u8>>, _>(key) {
        return value.map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    }
    if let Ok(value) = row.try_get::<Option<i64>, _>(key) {
        return value.map(|v| v.to_string());
    }
    if let Ok(value) = row.try_get::<Option<u64>, _>(key) {
        return value.map(|v| v.to_string());
    }
    None
}

/// Première colonne qui existe parmi `names` (le nom change entre les versions).
fn named(row: &MySqlRow, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| cell(row, *name))
}

async fn rows(
    conn: &mut MySqlConnection,
    sql: &'static str,
    deadline: Deadline,
) -> Option<Vec<MySqlRow>> {
    if deadline.expired() {
        return None;
    }
    match deadline.wait(sqlx::query(sql).fetch_all(&mut *conn)).await {
        Ok(Ok(rows)) => Some(rows),
        Ok(Err(error)) => {
            debug!(%error, sql, "lecture des statistiques MySQL refusée ou indisponible");
            None
        }
        Err(_) => None,
    }
}

fn pairs(rows: &[MySqlRow]) -> HashMap<String, String> {
    rows.iter().filter_map(|row| Some((cell(row, 0)?, cell(row, 1)?))).collect()
}

fn channels(rows: &[MySqlRow]) -> Vec<Channel> {
    rows.iter()
        .map(|row| Channel {
            name: named(row, &["Channel_Name", "Connection_name"]).unwrap_or_default(),
            io_running: named(row, &["Replica_IO_Running", "Slave_IO_Running"])
                .is_some_and(|v| is_yes(&v)),
            sql_running: named(row, &["Replica_SQL_Running", "Slave_SQL_Running"])
                .is_some_and(|v| is_yes(&v)),
            lag_seconds: named(row, &["Seconds_Behind_Source", "Seconds_Behind_Master"])
                .and_then(|v| v.trim().parse().ok()),
        })
        .collect()
}

/// Lit les statistiques du serveur sur `conn`, dans la limite de `budget`.
///
/// Toujours un `Vec`, jamais une erreur : voir la doc du module.
pub async fn collect(conn: &mut MySqlConnection, ts_ms: i64, budget: Duration) -> Vec<Sample> {
    let deadline = Deadline::starting_now(budget);
    let mut out = Vec::new();

    let status = rows(conn, STATUS, deadline).await.map(|r| pairs(&r)).unwrap_or_default();
    let variables = rows(conn, VARIABLES, deadline).await.map(|r| pairs(&r)).unwrap_or_default();
    out.extend(server_samples(&status, &variables, ts_ms));

    // Un compte sans le droit de lecture de la réplication reçoit une erreur : la
    // lecture est omise, comme sur un serveur qui n'est pas un réplica (zéro ligne).
    let found = match rows(conn, REPLICA, deadline).await {
        Some(found) => Some(found),
        None => rows(conn, REPLICA_LEGACY, deadline).await,
    };
    if let Some(found) = found {
        out.extend(replication_samples(&channels(&found), ts_ms));
    }
    if let Some(found) = rows(conn, LONG_QUERIES, deadline).await
        && let Some(row) = found.first()
    {
        let count = cell(row, 0).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let longest = cell(row, 1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        out.extend(long_query_samples(count, longest, ts_ms));
    }
    if let Some(found) = rows(conn, SCHEMAS, deadline).await {
        let schemas: Vec<(String, f64)> = found
            .iter()
            .filter_map(|row| Some((cell(row, 0)?, cell(row, 1)?.parse().ok()?)))
            .collect();
        out.extend(schema_samples(&schemas, ts_ms));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn value(samples: &[Sample], name: &str) -> Option<f64> {
        samples.iter().find(|s| s.metric == name).map(|s| s.value)
    }

    #[test]
    fn les_connexions_et_le_pourcentage_viennent_du_statut_et_des_variables() {
        let status =
            map(&[("Uptime", "86400"), ("Threads_connected", "45"), ("Questions", "1000")]);
        let vars =
            map(&[("max_connections", "151"), ("version", "8.0.39"), ("read_only", "OFF")]);
        let samples = server_samples(&status, &vars, 0);
        let used = value(&samples, "mysql_connections_used_percent").unwrap();
        assert!((used - 29.801).abs() < 0.01, "{used}");
        assert_eq!(value(&samples, "mysql_uptime_seconds"), Some(86400.0));
        assert_eq!(value(&samples, "mysql_read_only"), Some(0.0));
        let questions = samples.iter().find(|s| s.metric == "mysql_queries_total").unwrap();
        assert_eq!(questions.value, 1000.0);
        let info = samples.iter().find(|s| s.metric == "mysql_version_info").unwrap();
        assert_eq!(info.labels["version"], "8.0.39");
    }

    #[test]
    fn un_max_connections_nul_ne_divise_pas_par_zero() {
        let status = map(&[("Threads_connected", "3")]);
        let samples = server_samples(&status, &map(&[("max_connections", "0")]), 0);
        assert!(value(&samples, "mysql_connections_used_percent").is_none());
    }

    #[test]
    fn le_taux_du_tampon_est_omis_sans_lecture() {
        let idle =
            map(&[("Innodb_buffer_pool_read_requests", "0"), ("Innodb_buffer_pool_reads", "0")]);
        assert_eq!(buffer_pool_hit_percent(&idle), None);
        let busy = map(&[
            ("Innodb_buffer_pool_read_requests", "10000"),
            ("Innodb_buffer_pool_reads", "25"),
        ]);
        assert_eq!(buffer_pool_hit_percent(&busy), Some(99.75));
    }

    #[test]
    fn la_replication_rompue_na_pas_de_retard_publie() {
        let channels = vec![
            Channel {
                name: "".into(),
                io_running: true,
                sql_running: true,
                lag_seconds: Some(12.0),
            },
            Channel { name: "b".into(), io_running: true, sql_running: false, lag_seconds: None },
        ];
        let samples = replication_samples(&channels, 0);
        assert_eq!(value(&samples, "mysql_replicas_configured"), Some(2.0));
        let lags: Vec<_> =
            samples.iter().filter(|s| s.metric == "mysql_replica_lag_seconds").collect();
        assert_eq!(lags.len(), 1);
        let running: Vec<f64> = samples
            .iter()
            .filter(|s| s.metric == "mysql_replica_running")
            .map(|s| s.value)
            .collect();
        assert_eq!(running, vec![1.0, 0.0]);
    }

    #[test]
    fn un_serveur_sans_replication_dit_zero_canal() {
        let samples = replication_samples(&[], 0);
        assert_eq!(value(&samples, "mysql_replicas_configured"), Some(0.0));
        assert_eq!(samples.len(), 1);
    }

    #[test]
    fn les_schemas_sont_bornes_et_etiquetes() {
        let many: Vec<(String, f64)> = (0..15).map(|i| (format!("db{i}"), i as f64)).collect();
        let samples = schema_samples(&many, 0);
        assert_eq!(samples.len(), MAX_SCHEMAS);
        assert_eq!(samples[0].labels["database"], "db0");
    }

    #[test]
    fn les_etats_de_replication_se_lisent_en_yes_no() {
        assert!(is_yes("Yes"));
        assert!(!is_yes("Connecting"));
        assert!(!is_yes("No"));
    }
}
