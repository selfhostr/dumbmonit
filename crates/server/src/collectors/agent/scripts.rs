//! Contrôles personnalisés exécutés par un agent (`agent_script`) : un script
//! de l'administrateur ou un greffon Nagios, lancé sur la machine de l'agent.
//!
//! Le moniteur vit sur le serveur (programme, arguments, environnement, délai,
//! format), mais rien ne tourne ici : à chaque lot d'un agent, [`offered`]
//! relit les contrôles qui lui sont confiés (option `agent_id`) et l'accusé de
//! réception les lui remet ; le lot suivant rapporte leurs verdicts, que
//! [`record`] range. C'est l'agent qui décide de ce qu'il accepte d'exécuter
//! (voir `crates/agent/src/scripts/`) : le serveur ne fait que demander.
//!
//! # Verdict
//!
//! | Rendu par l'agent | Interrogation | Effet |
//! |---|---|---|
//! | OK | `Ok` | en service |
//! | WARNING | `Ok`, `script_warning = 1` | alerte `script_warning` (avertissement) |
//! | CRITICAL | `Unreachable` (message du contrôle) | hors service, alerte « injoignable » |
//! | UNKNOWN, refus, échec au lancement | `Config` | affiché, jamais notifié |
//! | délai dépassé | `Timeout` | hors service |
//! | plus de verdict depuis 3 périodes | `Unreachable` | hors service |
//!
//! # Mesures
//!
//! Écrites à la réception, à l'heure de l'exécution : `script_status` (0 à 3),
//! `script_warning` (0/1), `script_duration_seconds`, et pour chaque donnée de
//! performance `script_value{name="…"}` avec ses seuils
//! (`script_value_warning`, `script_value_critical`, `script_value_min`,
//! `script_value_max`) quand le contrôle les donne.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use dumbmonit_proto::script::{
    CHECK_DEFAULT_TIMEOUT_SECS, CHECK_MAX_TIMEOUT_SECS, ERROR_TIMEOUT, FORMAT_NAGIOS, FORMATS,
    MAX_SCRIPT_CHECKS, STATUS_CRITICAL, STATUS_OK, STATUS_UNKNOWN, STATUS_WARNING,
};
use dumbmonit_proto::{
    Collector, Credential, MetricKind, ProbeError, SCRIPT_CHECK_KIND, Sample, ScriptCheck,
    ScriptResult, Target, TargetId,
};
use sqlx::{Row, SqlitePool};

use crate::collectors::{MAX_BACKFILL_MS, MAX_SKEW_AHEAD_MS};
use crate::crypto::Cipher;
use crate::db;

/// Type des cibles « contrôle personnalisé ».
pub const KIND: &str = SCRIPT_CHECK_KIND;

/// Étiquettes d'options lues par ce module.
pub const OPTION_AGENT: &str = "agent_id";
pub const OPTION_COMMAND: &str = "command";
pub const OPTION_ARGS: &str = "args";
pub const OPTION_ENV: &str = "env";
pub const OPTION_TIMEOUT: &str = "timeout_seconds";
pub const OPTION_FORMAT: &str = "format";

/// Périodes sans verdict avant de déclarer le contrôle muet.
const MISSED_INTERVALS: u32 = 3;

/// Marge ajoutée au délai de fraîcheur : le verdict attend le lot suivant de
/// l'agent pour partir.
const DELIVERY_SLACK: Duration = Duration::from_secs(90);

/// Le contrôle décrit par les options d'une cible `agent_script`.
pub fn check_from_target(target: &Target) -> Result<ScriptCheck, ProbeError> {
    let tag = |key: &str| target.tags.get(key).map(|v| v.trim()).filter(|v| !v.is_empty());
    let config = |message: String| ProbeError::Config(message);
    let missing = "enter the command to run (a file of the agent's script directory)";
    let command = tag(OPTION_COMMAND).ok_or_else(|| config(missing.to_string()))?.to_string();
    let args = parse_args(tag(OPTION_ARGS).unwrap_or("")).map_err(config)?;
    let env = parse_env(tag(OPTION_ENV).unwrap_or("")).map_err(config)?;
    let timeout_secs = match tag(OPTION_TIMEOUT) {
        None => CHECK_DEFAULT_TIMEOUT_SECS,
        Some(raw) => parse_timeout(raw).ok_or_else(|| config(timeout_error(raw)))?,
    };
    let format = tag(OPTION_FORMAT).unwrap_or(FORMAT_NAGIOS).to_ascii_lowercase();
    if !FORMATS.contains(&format.as_str()) {
        return Err(config(format!("\"{OPTION_FORMAT}\" must be one of {}", FORMATS.join(", "))));
    }
    let check = ScriptCheck {
        id: target.id,
        command,
        args,
        env,
        timeout_secs,
        interval_secs: target.interval.as_secs(),
        format,
        secrets: secrets_of(&target.credential),
    };
    check.validate().map_err(config)?;
    Ok(check)
}

/// Refuse à l'enregistrement un contrôle que l'agent refuserait : l'erreur
/// dit quel réglage corriger, plutôt qu'un moniteur muet.
pub fn validate_tags(tags: &BTreeMap<String, String>) -> Result<(), String> {
    let target = Target {
        id: 0,
        name: String::new(),
        address: String::new(),
        kind: KIND.into(),
        profile_id: None,
        parent_id: None,
        interval: Duration::from_secs(60),
        enabled: true,
        tags: tags.clone(),
        credential: Credential::None,
        group_name: String::new(),
        position: 0,
    };
    agent_of(&target).map_err(|error| error.to_string())?;
    check_from_target(&target).map(|_| ()).map_err(|error| error.to_string())
}

fn parse_timeout(raw: &str) -> Option<u64> {
    raw.parse::<u64>().ok().filter(|secs| (1..=CHECK_MAX_TIMEOUT_SECS).contains(secs))
}

fn timeout_error(raw: &str) -> String {
    format!("\"{OPTION_TIMEOUT}\" must be 1 to {CHECK_MAX_TIMEOUT_SECS} seconds, not \"{raw}\"")
}

/// L'agent choisi pour une cible, tel que l'option l'enregistre.
pub fn agent_of(target: &Target) -> Result<TargetId, ProbeError> {
    target
        .tags
        .get(OPTION_AGENT)
        .and_then(|raw| raw.trim().parse::<TargetId>().ok())
        .ok_or_else(|| ProbeError::Config("choose the agent that runs this check".to_string()))
}

/// Les arguments : un tableau JSON de chaînes, ou de lignes `{"value": "…"}`
/// (le formulaire). Vide : aucun argument.
pub fn parse_args(raw: &str) -> Result<Vec<String>, String> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<serde_json::Value> = serde_json::from_str(raw)
        .map_err(|_| "\"args\" must be a JSON list of arguments".to_string())?;
    rows.iter()
        .map(|row| match row {
            serde_json::Value::String(arg) => Ok(arg.clone()),
            serde_json::Value::Object(fields) => fields
                .get("value")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or_else(|| "each argument row needs a \"value\"".to_string()),
            _ => Err("each argument must be a string".to_string()),
        })
        .collect()
}

/// L'environnement : un objet JSON `{"NOM": "valeur"}`, ou des lignes
/// `{"name": "…", "value": "…"}` (le formulaire).
pub fn parse_env(raw: &str) -> Result<BTreeMap<String, String>, String> {
    if raw.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|_| "\"env\" must be a JSON list of variables".to_string())?;
    let mut env = BTreeMap::new();
    match value {
        serde_json::Value::Object(map) => {
            for (name, value) in map {
                let value = value.as_str().ok_or("environment values must be strings")?;
                env.insert(name, value.to_string());
            }
        }
        serde_json::Value::Array(rows) => {
            for row in rows {
                let name = row.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
                let value = row.get("value").and_then(|v| v.as_str()).unwrap_or("");
                if name.is_empty() {
                    return Err("each environment row needs a \"name\"".to_string());
                }
                env.insert(name.to_string(), value.to_string());
            }
        }
        _ => return Err("\"env\" must be a JSON list of variables".to_string()),
    }
    Ok(env)
}

/// Les identifiants du moniteur, tels que l'agent les remplace dans les
/// arguments (`{{secret.password}}`).
fn secrets_of(credential: &Credential) -> BTreeMap<String, String> {
    match credential {
        Credential::ApiToken { token } => BTreeMap::from([("token".to_string(), token.clone())]),
        Credential::UsernamePassword { username, password } => BTreeMap::from([
            ("username".to_string(), username.clone()),
            ("password".to_string(), password.clone()),
        ]),
        _ => BTreeMap::new(),
    }
}

/// Un contrôle confié à un agent, avec sa cible (pour ses étiquettes).
#[derive(Debug, Clone)]
pub struct Offered {
    pub target: Target,
    pub check: ScriptCheck,
}

/// Les contrôles valides confiés à l'agent `agent_id`, et la trace de leur
/// remise : c'est elle qui distingue « pas encore exécuté » de « jamais reçu ».
/// Un contrôle mal réglé n'est pas remis ; son interrogation dit pourquoi.
pub async fn offered(
    pool: &SqlitePool,
    cipher: &Cipher,
    agent_id: TargetId,
    now_ms: i64,
) -> Result<Vec<Offered>> {
    let ids: Vec<TargetId> = sqlx::query_scalar(
        "SELECT id FROM targets
         WHERE kind = ? AND enabled = 1 AND json_extract(tags, '$.agent_id') = ?
         ORDER BY id LIMIT ?",
    )
    .bind(KIND)
    .bind(agent_id.to_string())
    .bind(MAX_SCRIPT_CHECKS as i64)
    .fetch_all(pool)
    .await
    .context("listing the custom checks of an agent")?;
    let mut offered = Vec::new();
    for id in ids {
        let Some(target) = db::targets::get(pool, cipher, id).await? else { continue };
        let Ok(check) = check_from_target(&target) else { continue };
        // Un changement d'agent efface le verdict de l'ancien.
        sqlx::query(
            "INSERT INTO agent_script_results (target_id, agent_id, offered_at_ms)
             VALUES (?, ?, ?)
             ON CONFLICT (target_id) DO UPDATE SET
                agent_id = excluded.agent_id, offered_at_ms = excluded.offered_at_ms,
                received_at_ms = NULL, ran_at_ms = NULL, status = NULL, message = '',
                error = NULL, duration_ms = NULL
             WHERE agent_script_results.agent_id <> excluded.agent_id",
        )
        .bind(id)
        .bind(agent_id)
        .bind(now_ms)
        .execute(pool)
        .await
        .context("recording a custom check handed to its agent")?;
        offered.push(Offered { target, check });
    }
    Ok(offered)
}

/// Range les verdicts d'un lot et rend les mesures à écrire.
///
/// Seuls comptent les verdicts des contrôles confiés à *cet* agent : une
/// machine ne peut pas écrire dans le moniteur d'une autre en forgeant un
/// identifiant.
pub async fn record(
    pool: &SqlitePool,
    agent_id: TargetId,
    offered: &[Offered],
    results: Vec<ScriptResult>,
    received_at_ms: i64,
) -> Result<Vec<Sample>> {
    let mut samples = Vec::new();
    for result in results.into_iter().take(MAX_SCRIPT_CHECKS) {
        let Some(entry) = offered.iter().find(|o| o.check.id == result.id) else {
            tracing::debug!(agent = agent_id, check = result.id, "verdict for an unknown check");
            continue;
        };
        let result = result.sanitized();
        sqlx::query(
            "UPDATE agent_script_results
             SET received_at_ms = ?, ran_at_ms = ?, status = ?, message = ?, error = ?,
                 duration_ms = ?
             WHERE target_id = ? AND agent_id = ?",
        )
        .bind(received_at_ms)
        .bind(result.ran_at_ms)
        .bind(i64::from(result.status))
        .bind(&result.message)
        .bind(result.error.as_deref())
        .bind(i64::try_from(result.duration_ms).unwrap_or(i64::MAX))
        .bind(entry.target.id)
        .bind(agent_id)
        .execute(pool)
        .await
        .context("recording a custom check verdict")?;
        samples.extend(samples_for(&entry.target, &result, received_at_ms));
    }
    Ok(samples)
}

/// Les mesures d'un verdict, étiquetées comme la cible du contrôle.
pub fn samples_for(target: &Target, result: &ScriptResult, received_at_ms: i64) -> Vec<Sample> {
    let options = crate::api::collector_option_keys(KIND);
    let base = target.base_labels_except(|key| options.iter().any(|option| option == key));
    // L'heure de l'exécution, bornée comme pour toute mesure d'agent.
    let (oldest, newest) = (received_at_ms - MAX_BACKFILL_MS, received_at_ms + MAX_SKEW_AHEAD_MS);
    let ts = result.ran_at_ms.clamp(oldest, newest);
    let gauge = |metric: &str, value: f64| Sample::new(metric, value, MetricKind::Gauge, ts);
    let mut samples = vec![gauge("script_status", f64::from(result.status.min(STATUS_UNKNOWN)))];
    let warning = result.error.is_none() && result.status == STATUS_WARNING;
    samples.push(gauge("script_warning", if warning { 1.0 } else { 0.0 }));
    if result.error.is_none() {
        samples.push(gauge("script_duration_seconds", result.duration_ms as f64 / 1000.0));
    }
    for metric in &result.metrics {
        let kind = if metric.counter { MetricKind::Counter } else { MetricKind::Gauge };
        let labelled = |name: &str, value: f64, kind: MetricKind| {
            let sample = Sample::new(name, value, kind, ts).with_label("name", metric.name.clone());
            match metric.unit.as_str() {
                "" => sample,
                unit => sample.with_label("unit", unit),
            }
        };
        samples.push(labelled("script_value", metric.value, kind));
        for (name, bound) in [
            ("script_value_warning", metric.warn),
            ("script_value_critical", metric.crit),
            ("script_value_min", metric.min),
            ("script_value_max", metric.max),
        ] {
            if let Some(bound) = bound {
                samples.push(labelled(name, bound, MetricKind::Gauge));
            }
        }
    }
    for sample in &mut samples {
        dumbmonit_proto::apply_identity(&base, &mut sample.labels);
    }
    samples.retain(|sample| sample.value.is_finite());
    samples
}

/// Délai au-delà duquel un contrôle sans nouveau verdict est déclaré muet.
pub fn deadline(interval: Duration, timeout: Duration) -> Duration {
    interval * MISSED_INTERVALS + timeout + DELIVERY_SLACK
}

/// Le message d'un CRITICAL tel qu'affiché : celui du contrôle, précédé du
/// statut quand il ne le dit pas lui-même.
fn critical_message(message: &str) -> String {
    if message.is_empty() {
        "the check returned CRITICAL".to_string()
    } else if message.to_ascii_uppercase().contains("CRIT") {
        message.to_string()
    } else {
        format!("CRITICAL: {message}")
    }
}

/// Le dernier état connu d'un contrôle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub agent_id: TargetId,
    pub offered_at_ms: i64,
    pub received_at_ms: Option<i64>,
    pub status: Option<u8>,
    pub message: String,
    pub error: Option<String>,
}

async fn stored(pool: &SqlitePool, target: TargetId) -> Result<Option<Stored>> {
    let row = sqlx::query(
        "SELECT agent_id, offered_at_ms, received_at_ms, status, message, error
         FROM agent_script_results WHERE target_id = ?",
    )
    .bind(target)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let status: Option<i64> = row.try_get("status")?;
    Ok(Some(Stored {
        agent_id: row.try_get("agent_id")?,
        offered_at_ms: row.try_get("offered_at_ms")?,
        received_at_ms: row.try_get("received_at_ms")?,
        status: status.and_then(|s| u8::try_from(s).ok()),
        message: row.try_get("message")?,
        error: row.try_get("error")?,
    }))
}

/// Ce que l'interrogation conclut d'un état relu, à l'instant `now_ms`.
/// Pure : c'est elle que les tests exercent.
pub fn verdict(
    stored: Option<&Stored>,
    agent_id: TargetId,
    updated_at_ms: i64,
    deadline: Duration,
    timeout: Duration,
    now_ms: i64,
) -> Result<Vec<Sample>, ProbeError> {
    let age = |since: i64| Duration::from_millis(now_ms.saturating_sub(since).max(0) as u64);
    let Some(stored) = stored.filter(|s| s.agent_id == agent_id) else {
        // Pas encore remis à l'agent : on lui laisse le temps d'un lot.
        if age(updated_at_ms) <= deadline {
            return Ok(Vec::new());
        }
        return Err(ProbeError::Unreachable(format!(
            "the agent has not collected this check for {} s: is it running and connected?",
            age(updated_at_ms).as_secs()
        )));
    };
    let Some(received_at_ms) = stored.received_at_ms else {
        if age(stored.offered_at_ms) <= deadline {
            return Ok(Vec::new());
        }
        return Err(ProbeError::Unreachable(format!(
            "the agent received this check {} s ago but never reported it: update the agent \
             (custom checks need a recent version) and look at its log",
            age(stored.offered_at_ms).as_secs()
        )));
    };
    let silent = age(received_at_ms);
    if silent > deadline {
        return Err(ProbeError::Unreachable(format!(
            "no result from the agent for {} s (limit {} s)",
            silent.as_secs(),
            deadline.as_secs()
        )));
    }
    let freshness = Sample::new(
        "script_last_result_age_seconds",
        silent.as_secs_f64(),
        MetricKind::Gauge,
        now_ms,
    );
    let message = stored.message.clone();
    match (stored.error.as_deref(), stored.status.unwrap_or(STATUS_UNKNOWN)) {
        (Some(ERROR_TIMEOUT), _) => Err(ProbeError::Timeout(timeout)),
        (Some(_), _) => Err(ProbeError::Config(message)),
        (None, STATUS_OK | STATUS_WARNING) => Ok(vec![freshness]),
        (None, STATUS_CRITICAL) => Err(ProbeError::Unreachable(critical_message(&message))),
        (None, _) if message.is_empty() => {
            Err(ProbeError::Config("the check returned UNKNOWN".to_string()))
        }
        (None, _) => Err(ProbeError::Config(format!("UNKNOWN: {message}"))),
    }
}

/// Le collecteur : il n'exécute rien, il relit le dernier verdict.
pub struct AgentScriptCollector {
    pool: SqlitePool,
}

impl AgentScriptCollector {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Collector for AgentScriptCollector {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn probe(&self, target: &Target) -> Result<Vec<Sample>, ProbeError> {
        let check = check_from_target(target)?;
        let agent_id = agent_of(target)?;
        let agent_kind: Option<String> =
            sqlx::query_scalar("SELECT kind FROM targets WHERE id = ?")
                .bind(agent_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| ProbeError::Other(e.into()))?;
        if agent_kind.as_deref() != Some("agent") {
            return Err(ProbeError::Config(format!(
                "the agent chosen for this check (#{agent_id}) no longer exists: choose another one"
            )));
        }
        let updated_at: Option<String> =
            sqlx::query_scalar("SELECT updated_at FROM targets WHERE id = ?")
                .bind(target.id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| ProbeError::Other(e.into()))?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let updated_at_ms = updated_at
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok())
            .map_or(now_ms, |at| at.timestamp_millis());
        let stored = stored(&self.pool, target.id).await.map_err(ProbeError::Other)?;
        let deadline = deadline(check.interval(), check.timeout());
        verdict(stored.as_ref(), agent_id, updated_at_ms, deadline, check.timeout(), now_ms)
    }
}

#[cfg(test)]
mod tests;
