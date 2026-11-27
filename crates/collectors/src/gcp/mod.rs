//! Google Cloud, par les API REST de Compute Engine, Cloud SQL et Cloud Monitoring.
//!
//! Trois lectures sur un projet, avec un compte de service (clé JSON) qui n'a
//! que des rôles de lecture (`Compute Viewer`, `Cloud SQL Viewer`,
//! `Monitoring Viewer`) :
//!
//! * **Compute Engine** (`aggregated/instances`) : l'état de chaque machine.
//!   Lecture principale : si elle échoue, la sonde échoue ;
//! * **Cloud SQL** (`instances`) : l'état de chaque instance. Facultative ;
//! * **Cloud Monitoring** (`timeSeries`) : la dernière valeur des métriques
//!   choisies (par défaut l'utilisation du processeur). Facultative.
//!
//! Une machine arrêtée (`TERMINATED`, `SUSPENDED`) ou une instance SQL que son
//! propriétaire a arrêtée (`SUSPENDED`) n'est pas une panne : elle est comptée
//! à part. Facturation : volontairement non lue.
//!
//! Authentification : flux OAuth2 « JWT bearer » (RFC 7523). Le jeton JWT est
//! signé RS256 avec la clé privée du compte de service (`ring`), échangé contre
//! un jeton d'accès d'une heure gardé jusqu'à son expiration. Le point d'accès
//! du jeton est celui de Google, jamais le `token_uri` de la clé, sauf
//! réglage explicite `token_url`.
//!
//! # Réglages, portés par les étiquettes de la cible
//!
//! | Étiquette | Défaut | Rôle |
//! |---|---|---|
//! | `project_id` | `project_id` de la clé | Projet à surveiller. |
//! | `metrics` | `compute.googleapis.com/instance/cpu/utilization` | Types de métriques Monitoring, séparés par des virgules. |
//! | `api_url` | hôtes Google | Racine unique des trois API (relais, tests). |
//! | `token_url` | `https://oauth2.googleapis.com/token` | Point d'accès du jeton. |
//! | `request_timeout_seconds` | `15` | Délai par requête HTTP. |

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use dumbmonit_proto::{Collector, Credential, ProbeError, Sample, Target};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::cloud;
use crate::rest::{MAX_NAMED, RestClient, TokenCache, flag, gauge, now_ms, text};
use crate::selfhosted::options::tag;

pub const DEFAULT_PORT: u16 = 443;
pub const DEFAULT_ADDRESS: &str = "googleapis.com";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const COMPUTE_URL: &str = "https://compute.googleapis.com";
const SQL_URL: &str = "https://sqladmin.googleapis.com";
const MONITORING_URL: &str = "https://monitoring.googleapis.com";
const SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const DEFAULT_METRIC: &str = "compute.googleapis.com/instance/cpu/utilization";
/// Pages lues au plus par liste : un gros projet ne bloque pas la sonde.
const MAX_PAGES: usize = 20;
/// Types de métriques lus au plus par sonde.
const MAX_METRICS: usize = 5;
const PREFIX: &str = "gcp";

#[derive(Default)]
pub struct GcpCollector {
    tokens: TokenCache,
}

/// Les champs utiles de la clé JSON d'un compte de service.
#[derive(Deserialize)]
struct ServiceKey {
    client_email: String,
    private_key: String,
    #[serde(default)]
    project_id: String,
}

struct Settings {
    project: String,
    metrics: Vec<String>,
    api_url: Option<String>,
    token_url: String,
}

impl Settings {
    fn from_target(target: &Target, key: &ServiceKey) -> Result<Self, ProbeError> {
        let project = tag(target, "project_id").unwrap_or(&key.project_id);
        let project = cloud::url_segment("project_id", project)?;
        let mut metrics: Vec<String> = tag(target, "metrics")
            .unwrap_or(DEFAULT_METRIC)
            .split(',')
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .collect();
        metrics.truncate(MAX_METRICS);
        let url = |name: &str| -> Result<Option<String>, ProbeError> {
            match tag(target, name) {
                None => Ok(None),
                Some(url) if url.starts_with("https://") || url.starts_with("http://") => {
                    Ok(Some(url.trim_end_matches('/').to_string()))
                }
                Some(url) => Err(ProbeError::Config(format!(
                    "Invalid {name} \"{url}\": expected a full URL such as {TOKEN_URL}"
                ))),
            }
        };
        Ok(Self {
            project,
            metrics,
            api_url: url("api_url")?,
            token_url: url("token_url")?.unwrap_or_else(|| TOKEN_URL.to_string()),
        })
    }

    fn base<'a>(&'a self, default: &'a str) -> &'a str {
        self.api_url.as_deref().unwrap_or(default)
    }
}

/// Clé JSON du compte de service, lue dans le secret de la cible.
fn service_key(target: &Target) -> Result<ServiceKey, ProbeError> {
    let raw = match &target.credential {
        Credential::ApiToken { token } if !token.trim().is_empty() => token.trim(),
        other => {
            return Err(ProbeError::Config(format!(
                "Google Cloud expects the JSON key of a service account, configured: {other}"
            )));
        }
    };
    let key: ServiceKey = serde_json::from_str(raw).map_err(|_| {
        ProbeError::Config(
            "The service account key is not valid JSON: paste the whole key file, which holds \
             client_email and private_key."
                .to_string(),
        )
    })?;
    if key.client_email.trim().is_empty() || key.private_key.trim().is_empty() {
        return Err(ProbeError::Config(
            "The service account key lacks client_email or private_key.".to_string(),
        ));
    }
    Ok(key)
}

/// JWT RS256 (RFC 7523) signé avec la clé privée PKCS#8 du compte de service.
fn signed_jwt(key: &ServiceKey, audience: &str, now_s: i64) -> Result<String, ProbeError> {
    let pem: String =
        key.private_key.lines().filter(|l| !l.trim_start().starts_with("-----")).collect();
    let der = STANDARD.decode(pem.trim()).map_err(|_| {
        ProbeError::Config("The private_key of the service account key is not valid PEM.".into())
    })?;
    let pair = ring::signature::RsaKeyPair::from_pkcs8(&der).map_err(|_| {
        ProbeError::Config(
            "The private_key of the service account key is not an RSA PKCS#8 key.".into(),
        )
    })?;
    let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = serde_json::json!({
        "iss": key.client_email,
        "scope": SCOPE,
        "aud": audience,
        "iat": now_s,
        "exp": now_s + 3600,
    });
    let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
    let signing_input = format!("{header}.{claims}");
    let mut signature = vec![0u8; pair.public().modulus_len()];
    pair.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        signing_input.as_bytes(),
        &mut signature,
    )
    .map_err(|_| ProbeError::Config("The service account key could not sign the token.".into()))?;
    Ok(format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

/// Ce que la sonde a lu : Compute, puis Cloud SQL et Monitoring s'ils se lisent.
struct Snapshot {
    compute: Vec<Value>,
    sql: Option<Vec<Value>>,
    /// Type de métrique → séries lues ; `None` : Monitoring illisible.
    series: Option<Vec<(String, Vec<Value>)>>,
}

enum Listing {
    Items(Vec<Value>),
    Refused(StatusCode, String, String),
}

impl GcpCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Jeton d'accès : gardé jusqu'à expiration, sinon échangé contre un JWT signé.
    async fn token(
        &self,
        target: &Target,
        key: &ServiceKey,
        settings: &Settings,
    ) -> Result<String, ProbeError> {
        let cache_key = format!("{}:{}", target.id, key.client_email);
        if let Some(token) = self.tokens.get(&cache_key) {
            return Ok(token);
        }
        let assertion = signed_jwt(key, &settings.token_url, now_ms() / 1000)?;
        let login = RestClient::for_base(target, &settings.token_url, "Google")?;
        let form = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", assertion.as_str()),
        ];
        let (status, body) = login.post_form("", &form).await?;
        let reply: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        if status.is_success() {
            let token = text(&reply, "access_token").ok_or_else(|| {
                ProbeError::Protocol(
                    "Google returned no access_token for the service account".into(),
                )
            })?;
            let lifetime = reply.get("expires_in").and_then(Value::as_u64).unwrap_or(3600);
            self.tokens.put(&cache_key, token, Duration::from_secs(lifetime));
            return Ok(token.to_string());
        }
        let code = text(&reply, "error").unwrap_or_default();
        let description = text(&reply, "error_description").unwrap_or_default();
        if matches!(status.as_u16(), 400 | 401 | 403) {
            return Err(ProbeError::Auth(format!(
                "Google refused the service account ({status}, {code}): check that the key \
                 has not been deleted and that the clock is right. {description}"
            )));
        }
        Err(login.status_error(status, &body, "token"))
    }

    async fn snapshot(&self, target: &Target) -> Result<Snapshot, ProbeError> {
        let key = service_key(target)?;
        let settings = Settings::from_target(target, &key)?;
        let token = self.token(target, &key, &settings).await?;
        let project = &settings.project;
        let client = |default: &str| -> Result<RestClient, ProbeError> {
            let mut client = RestClient::for_base(target, settings.base(default), "Google Cloud")?;
            client.set_header("Authorization", format!("Bearer {token}"));
            Ok(client)
        };

        let compute = client(COMPUTE_URL)?;
        let first = format!("/compute/v1/projects/{project}/aggregated/instances?maxResults=500");
        let compute_items = match list_all(&compute, first, "items").await? {
            Listing::Items(items) => items,
            Listing::Refused(status, body, path) => {
                if status == StatusCode::UNAUTHORIZED {
                    self.tokens.forget(&format!("{}:{}", target.id, key.client_email));
                }
                return Err(compute_error(&compute, project, status, &body, &path));
            }
        };

        let sql = client(SQL_URL)?;
        let sql_path = format!("/sql/v1beta4/projects/{project}/instances");
        let sql_items = match list_all(&sql, sql_path, "items").await? {
            Listing::Items(items) => Some(items),
            Listing::Refused(..) => None,
        };

        let monitoring = client(MONITORING_URL)?;
        let mut series = Some(Vec::new());
        for metric in &settings.metrics {
            match read_series(&monitoring, project, metric, now_ms()).await? {
                Some(found) => {
                    if let Some(all) = series.as_mut() {
                        all.push((metric.clone(), found));
                    }
                }
                None => {
                    series = None;
                    break;
                }
            }
        }
        Ok(Snapshot { compute: flatten_compute(&compute_items), sql: sql_items, series })
    }
}

fn compute_error(
    client: &RestClient,
    project: &str,
    status: StatusCode,
    body: &str,
    path: &str,
) -> ProbeError {
    match status.as_u16() {
        401 => ProbeError::Auth("Google refused the access token (401).".to_string()),
        403 => ProbeError::Auth(format!(
            "Google refused to list the Compute Engine instances of project \"{project}\" \
             (403): give the service account the Compute Viewer role and enable the Compute \
             Engine API."
        )),
        400 | 404 => ProbeError::Config(format!(
            "Google knows no project \"{project}\" for this service account ({status}): copy \
             the project ID, not its name."
        )),
        _ => client.status_error(status, body, path),
    }
}

/// Lit une liste paginée (`nextPageToken`) ; `field` porte les éléments.
async fn list_all(client: &RestClient, first: String, field: &str) -> Result<Listing, ProbeError> {
    let mut items = Vec::new();
    let mut path = first.clone();
    let sep = if first.contains('?') { '&' } else { '?' };
    for _ in 0..MAX_PAGES {
        let (status, body) = client.get(&path).await?;
        if !status.is_success() {
            return Ok(Listing::Refused(status, body, path));
        }
        let page: Value = client.decode(&body, &path)?;
        match page.get(field) {
            Some(Value::Array(values)) => items.extend(values.iter().cloned()),
            // `aggregated/instances` : un objet par zone, chacun avec `instances`.
            Some(Value::Object(zones)) => items.extend(zones.values().cloned()),
            _ => {}
        }
        match text(&page, "nextPageToken") {
            Some(next) => path = format!("{first}{sep}pageToken={}", encode(next)),
            None => break,
        }
    }
    Ok(Listing::Items(items))
}

/// Aplatit les groupes par zone de `aggregated/instances` en une liste de machines.
fn flatten_compute(zones: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for zone in zones {
        if let Some(list) = zone.get("instances").and_then(Value::as_array) {
            out.extend(list.iter().cloned());
        }
    }
    out
}

/// Dernière valeur de chaque série d'un type de métrique ; `None` si Monitoring refuse.
async fn read_series(
    client: &RestClient,
    project: &str,
    metric: &str,
    now: i64,
) -> Result<Option<Vec<Value>>, ProbeError> {
    let end = chrono::DateTime::from_timestamp_millis(now).unwrap_or_default();
    let start = end - chrono::Duration::minutes(10);
    let filter = format!("metric.type=\"{metric}\"");
    let path = format!(
        "/v3/projects/{project}/timeSeries?filter={}&interval.startTime={}&interval.endTime={}\
         &aggregation.alignmentPeriod=300s&aggregation.perSeriesAligner=ALIGN_MEAN&view=FULL",
        encode(&filter),
        encode(&start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        encode(&end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    );
    let (status, body) = client.get(&path).await?;
    if !status.is_success() {
        return Ok(None);
    }
    let page: Value = client.decode(&body, &path)?;
    Ok(Some(page.get("timeSeries").and_then(Value::as_array).cloned().unwrap_or_default()))
}

/// Encodage pourcent d'une valeur de requête.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[async_trait]
impl Collector for GcpCollector {
    fn kind(&self) -> &'static str {
        "gcp"
    }

    async fn probe(&self, target: &Target) -> Result<Vec<Sample>, ProbeError> {
        let snapshot = self.snapshot(target).await?;
        Ok(samples(&snapshot, now_ms()))
    }

    async fn discover(&self, target: &Target) -> Result<Option<String>, ProbeError> {
        self.snapshot(target).await?;
        Ok(Some("gcp".to_string()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Health {
    Running,
    /// Démarrage, arrêt ou maintenance en cours : ni normal ni encore une panne.
    Transition,
    /// Arrêtée par son propriétaire : pas une panne.
    Stopped,
    Failed,
}

fn classify_compute(status: &str) -> Health {
    match status {
        "RUNNING" => Health::Running,
        "TERMINATED" | "STOPPED" | "SUSPENDED" => Health::Stopped,
        "REPAIRING" => Health::Failed,
        _ => Health::Transition,
    }
}

fn classify_sql(state: &str) -> Health {
    match state {
        "RUNNABLE" => Health::Running,
        "SUSPENDED" | "STOPPED" => Health::Stopped,
        "FAILED" | "UNKNOWN_STATE" => Health::Failed,
        _ => Health::Transition,
    }
}

/// Dernier segment d'une URL de ressource (`…/zones/europe-west1-b` → `europe-west1-b`).
fn last_segment(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

fn samples(snapshot: &Snapshot, ts_ms: i64) -> Vec<Sample> {
    let g = |name: &str, value: f64| gauge(&format!("{PREFIX}_{name}"), value, ts_ms);
    let mut out = Vec::new();
    let mut counts: BTreeMap<&str, f64> = BTreeMap::new();
    let mut named = 0usize;
    for vm in &snapshot.compute {
        let name = text(vm, "name").unwrap_or("instance");
        let state = text(vm, "status").unwrap_or("UNKNOWN");
        let verdict = classify_compute(state);
        let slot = match verdict {
            Health::Running => "running",
            Health::Stopped => "stopped",
            Health::Failed => "failed",
            Health::Transition => "transition",
        };
        *counts.entry(slot).or_default() += 1.0;
        if verdict == Health::Running {
            continue;
        }
        named += 1;
        if named <= MAX_NAMED {
            let zone = text(vm, "zone").map(last_segment).unwrap_or_default();
            let ok = verdict != Health::Failed;
            out.push(cloud::item(PREFIX, "Compute Engine", name, ok, state, ts_ms));
            if verdict == Health::Failed {
                out.push(
                    g("instance_unhealthy", 1.0)
                        .with_label("instance", name)
                        .with_label("zone", zone)
                        .with_label("state", state),
                );
            }
        }
    }
    let total = snapshot.compute.len() as f64;
    out.push(g("instances", total));
    out.push(g("instances_running", counts.get("running").copied().unwrap_or(0.0)));
    out.push(g("instances_stopped", counts.get("stopped").copied().unwrap_or(0.0)));
    out.push(g("instances_failed", counts.get("failed").copied().unwrap_or(0.0)));
    out.push(cloud::figure(PREFIX, "Instances", "", total, ts_ms));
    let failed = counts.get("failed").copied().unwrap_or(0.0);
    out.push(cloud::figure(PREFIX, "Failed", "", failed, ts_ms));

    out.push(flag(&format!("{PREFIX}_sql_readable"), snapshot.sql.is_some(), ts_ms));
    if let Some(sql) = &snapshot.sql {
        let (mut running, mut stopped, mut failed) = (0.0, 0.0, 0.0);
        for db in sql.iter().take(MAX_NAMED) {
            let name = text(db, "name").unwrap_or("sql");
            let state = text(db, "state").unwrap_or("UNKNOWN_STATE");
            let verdict = classify_sql(state);
            match verdict {
                Health::Running => running += 1.0,
                Health::Stopped => stopped += 1.0,
                Health::Failed => failed += 1.0,
                Health::Transition => {}
            }
            if verdict != Health::Running {
                let ok = verdict != Health::Failed;
                out.push(cloud::item(PREFIX, "Cloud SQL", name, ok, state, ts_ms));
            }
            if verdict == Health::Failed {
                out.push(
                    g("sql_unhealthy", 1.0).with_label("instance", name).with_label("state", state),
                );
            }
        }
        out.push(g("sql_instances", sql.len() as f64));
        out.push(g("sql_instances_running", running));
        out.push(g("sql_instances_stopped", stopped));
        out.push(g("sql_instances_failed", failed));
    }

    out.push(flag(&format!("{PREFIX}_monitoring_readable"), snapshot.series.is_some(), ts_ms));
    for (metric, series) in snapshot.series.iter().flatten() {
        for one in series.iter().take(MAX_NAMED) {
            let Some(value) = latest_value(one) else { continue };
            let resource = one
                .get("resource")
                .and_then(|r| r.get("labels"))
                .and_then(|l| text(l, "instance_id").or_else(|| text(l, "database_id")))
                .unwrap_or("project");
            let name = one
                .get("metric")
                .and_then(|m| m.get("labels"))
                .and_then(|l| text(l, "instance_name"))
                .unwrap_or(resource);
            // Un ratio 0..1 (utilisation) s'affiche en pour cent.
            let (value, unit) = if metric.ends_with("/utilization") {
                (value * 100.0, "%")
            } else {
                (value, "")
            };
            out.push(
                g("metric", value)
                    .with_label("metric", metric.as_str())
                    .with_label("resource", name)
                    .with_label("unit", unit),
            );
        }
    }
    out
}

/// Dernier point d'une série (Monitoring les renvoie du plus récent au plus ancien).
fn latest_value(series: &Value) -> Option<f64> {
    let point = series.get("points")?.as_array()?.first()?;
    let value = point.get("value")?;
    value
        .get("doubleValue")
        .and_then(Value::as_f64)
        .or_else(|| text(value, "int64Value").and_then(|v| v.parse().ok()))
}

#[cfg(test)]
mod tests;
