//! qBittorrent (WebUI API v2).
//!
//! L'API ouvre une session par cookie : `POST /api/v2/auth/login` avec le
//! compte de l'interface web, puis le cookie `SID` accompagne chaque requête.
//! Sans identifiants, la sonde essaie quand même : qBittorrent peut sauter
//! l'authentification pour un sous-réseau ou pour `localhost` (option « Bypass
//! authentication for clients on whitelisted IP subnets »).
//!
//! Un seul appel, `GET /api/v2/sync/maindata?rid=0`, rend tout d'un coup : l'état
//! du serveur (vitesses, connectivité, espace libre) et la liste des torrents
//! avec leur état. Compter les états suffit à voir ce qui coince.
//!
//! Les pannes que ce module existe pour voir : **torrents en erreur** (disque
//! plein, fichiers manquants après un déplacement) qui ne reprennent jamais
//! seuls, **connectivité perdue** (port fermé, VPN tombé, « firewalled »), et
//! **espace libre** du dossier de téléchargement.

use async_trait::async_trait;
use dumbmonit_proto::{Collector, Credential, ProbeError, Sample, Target};
use reqwest::StatusCode;
use serde_json::Value;

use super::client::{Auth, HttpClient, cookies_of};
use super::{MAX_NAMED, flag, gauge, http_client, metric, now_ms, number};

pub const DEFAULT_PORT: u16 = 8080;
const PRODUCT: &str = "qBittorrent";
const P: &str = "qbittorrent";

/// Longueur maximale d'un nom de torrent repris dans une étiquette.
const MAX_NAME: usize = 120;

/// Le groupe d'un état de torrent, tel que l'API le nomme (v4 et v5).
pub fn state_group(state: &str) -> &'static str {
    match state {
        "error" | "missingFiles" => "error",
        "downloading" | "forcedDL" | "metaDL" | "forcedMetaDL" | "allocating" => "downloading",
        "stalledDL" => "stalled_download",
        "uploading" | "forcedUP" | "stalledUP" => "seeding",
        "pausedDL" | "pausedUP" | "stoppedDL" | "stoppedUP" => "paused",
        "queuedDL" | "queuedUP" => "queued",
        "checkingDL" | "checkingUP" | "checkingResumeData" | "moving" => "checking",
        _ => "other",
    }
}

const GROUPS: [&str; 8] = [
    "downloading",
    "stalled_download",
    "seeding",
    "paused",
    "queued",
    "checking",
    "error",
    "other",
];

// ------------------------------------------------------------ traduction

/// `GET /api/v2/sync/maindata?rid=0` en métriques.
pub fn samples(version: &str, body: &Value, ts_ms: i64) -> Vec<Sample> {
    let mut out =
        vec![gauge(&metric(P, "version_info"), 1.0, ts_ms).with_label("version", version)];
    let state = body.get("server_state");
    let field = |key: &str| state.and_then(|s| number(s.get(key)));
    if let Some(status) = state.and_then(|s| s.get("connection_status")).and_then(Value::as_str) {
        out.push(
            flag(&metric(P, "connected"), status == "connected", ts_ms)
                .with_label("status", status),
        );
    }
    for (name, key) in [
        ("download_speed_bytes", "dl_info_speed"),
        ("upload_speed_bytes", "up_info_speed"),
        ("free_space_bytes", "free_space_on_disk"),
        ("dht_nodes", "dht_nodes"),
    ] {
        if let Some(value) = field(key) {
            out.push(gauge(&metric(P, name), value, ts_ms));
        }
    }

    let empty = serde_json::Map::new();
    let torrents = body.get("torrents").and_then(Value::as_object).unwrap_or(&empty);
    let mut counts = [0usize; GROUPS.len()];
    let mut named = 0usize;
    let mut errored: Vec<Sample> = Vec::new();
    for torrent in torrents.values() {
        let state = torrent.get("state").and_then(Value::as_str).unwrap_or("");
        let group = state_group(state);
        if let Some(index) = GROUPS.iter().position(|g| *g == group) {
            counts[index] += 1;
        }
        if group == "error" && named < MAX_NAMED {
            named += 1;
            let name: String = torrent
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unnamed")
                .chars()
                .take(MAX_NAME)
                .collect();
            errored.push(
                gauge(&metric(P, "torrent_error"), 1.0, ts_ms)
                    .with_label("torrent", &name)
                    .with_label("state", state),
            );
        }
    }
    out.push(gauge(&metric(P, "torrents_total"), torrents.len() as f64, ts_ms));
    for (group, count) in GROUPS.iter().zip(counts) {
        out.push(gauge(&metric(P, "torrents"), count as f64, ts_ms).with_label("state", *group));
    }
    out.push(gauge(&metric(P, "torrents_errored"), counts[6] as f64, ts_ms));
    out.extend(errored);
    out
}

// ------------------------------------------------------------------ sonde

const WRONG_LOGIN: &str = "qBittorrent refused the user name or password on \
    /api/v2/auth/login. Check the WebUI user name and password.";

/// Ouvre la session si des identifiants sont configurés, et rend le client
/// qui porte le cookie.
async fn session(target: &Target) -> Result<HttpClient, ProbeError> {
    let anonymous = http_client(target, "http", DEFAULT_PORT, Auth::default(), PRODUCT)?;
    let (username, password) = match &target.credential {
        Credential::UsernamePassword { username, password } => (username, password),
        Credential::None => return Ok(anonymous),
        other => {
            return Err(ProbeError::Config(format!(
                "qBittorrent expects a user name and password (or none, with authentication \
                 bypass), configured: {other}"
            )));
        }
    };
    let form = [("username", username.as_str()), ("password", password.as_str())];
    let path = "/api/v2/auth/login";
    let (status, headers, body) = anonymous.login(path, Some(&form[..]), None).await?;
    if status == StatusCode::FORBIDDEN {
        return Err(ProbeError::Auth(
            "qBittorrent banned this address after too many failed logins (403): wait, or \
             clear the ban in the WebUI settings."
                .to_string(),
        ));
    }
    // qBittorrent 5 répond 401 à un mauvais mot de passe (les versions
    // précédentes, 200 « Fails. ») : c'est un compte, pas un jeton.
    if status == StatusCode::UNAUTHORIZED || body.trim().eq_ignore_ascii_case("fails.") {
        return Err(ProbeError::Auth(WRONG_LOGIN.to_string()));
    }
    if !status.is_success() {
        return Err(anonymous.status_error(status, &body, path));
    }
    let cookie = cookies_of(&headers);
    if cookie.is_empty() {
        return Ok(anonymous);
    }
    http_client(target, "http", DEFAULT_PORT, Auth::header("Cookie", cookie), PRODUCT)
}

/// qBittorrent répond 403 (parfois 401) à toute requête sans session valable :
/// il n'y a pas de jeton à vérifier, c'est le compte du WebUI qu'il faut.
fn refusal(client: &HttpClient, status: StatusCode, body: &str, path: &str) -> ProbeError {
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return ProbeError::Auth(format!(
            "qBittorrent refused {path} ({}): log in with the WebUI user name and password, or \
             allow DumbMonit's address under Tools → Options → Web UI → Bypass authentication \
             for clients in whitelisted IP subnets.",
            status.as_u16()
        ));
    }
    client.status_error(status, body, path)
}

pub async fn probe(client: &HttpClient, ts_ms: i64) -> Result<Vec<Sample>, ProbeError> {
    // La preuve de vie et d'authentification : le seul appel qui condamne.
    let path = "/api/v2/app/version";
    let (status, body) = client.get(path).await?;
    if !status.is_success() {
        return Err(refusal(client, status, &body, path));
    }
    let version = body.trim().trim_matches('"');
    let maindata: Value = client.get_json("/api/v2/sync/maindata?rid=0").await?;
    Ok(samples(version, &maindata, ts_ms))
}

// ------------------------------------------------------------ collecteur

#[derive(Default)]
pub struct QbittorrentCollector;

impl QbittorrentCollector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Collector for QbittorrentCollector {
    fn kind(&self) -> &str {
        "qbittorrent"
    }

    async fn probe(&self, target: &Target) -> Result<Vec<Sample>, ProbeError> {
        probe(&session(target).await?, now_ms()).await
    }

    async fn discover(&self, target: &Target) -> Result<Option<String>, ProbeError> {
        let client = session(target).await?;
        let (status, body) = client.get("/api/v2/app/version").await?;
        if !status.is_success() {
            return Err(refusal(&client, status, &body, "/api/v2/app/version"));
        }
        Ok(Some("qbittorrent".to_string()))
    }
}

#[cfg(test)]
mod tests;
