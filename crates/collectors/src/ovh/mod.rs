//! OVHcloud, par l'API publique (`https://eu.api.ovh.com/1.0`).
//!
//! Deux familles de serveurs sont lues, chacune avec ses droits :
//!
//! * les VPS (`GET /vps`, puis `GET /vps/{nom}` pour l'état) ;
//! * les serveurs dédiés (`GET /dedicated/server`, puis `GET /dedicated/server/{nom}`
//!   pour l'état et la supervision d'OVHcloud).
//!
//! Pour chaque serveur, les tâches en attente ou en cours (`/vps/{nom}/tasks`,
//! `/dedicated/server/{nom}/task`) disent qu'une opération est en train de
//! toucher la machine (redémarrage, réinstallation, migration…). L'API publique
//! n'expose pas de calendrier des interventions planifiées par OVHcloud : ce
//! module ne peut donc pas annoncer une maintenance à l'avance, seulement la
//! constater quand elle a commencé.
//!
//! # Authentification
//!
//! L'API d'OVHcloud signe chaque requête. Le couple « clé d'application,
//! secret d'application » identifie l'application, la « clé de consommateur »
//! porte les droits accordés par le titulaire du compte. L'identifiant de la
//! cible tient donc en deux champs : le nom d'utilisateur est la clé
//! d'application, le mot de passe est `secret d'application:clé de
//! consommateur` (séparés par deux-points). Chaque requête porte
//! `X-Ovh-Application`, `X-Ovh-Consumer`, `X-Ovh-Timestamp` et
//! `X-Ovh-Signature` = `$1$` + SHA-1 de
//! `secret+consommateur+méthode+URL+corps+horodatage`. L'horodatage est celui
//! du serveur (`GET /auth/time`), pour ne pas dépendre de l'horloge locale.
//!
//! # Droits : un refus partiel n'est pas une panne
//!
//! Une clé de consommateur qui n'a le droit que sur `/vps*` reçoit un 403
//! « This call has not been granted » sur `/dedicated/server` : la famille est
//! omise et `ovh_dedicated_readable` vaut 0. La sonde n'échoue que si aucune
//! des deux familles n'est lisible, ou si l'identifiant est refusé.
//!
//! `GET /{famille}/{nom}/serviceInfos` donne la date d'expiration du service
//! (`ovh_service_expiry_seconds`, secondes jusqu'à minuit UTC de cette date), reprise
//! par `GET /api/deadlines` ; une lecture refusée ou absente est simplement omise.
//!
//! Les 30 premiers serveurs de chaque famille sont décrits ; les comptes
//! restent exacts au-delà.
//!
//! # Réglages, portés par les étiquettes de la cible
//!
//! | Étiquette | Défaut | Rôle |
//! |---|---|---|
//! | `request_timeout_seconds` | `15` | Délai par requête HTTP. |

use async_trait::async_trait;
use dumbmonit_proto::{Collector, Credential, ProbeError, Sample, Target};
use reqwest::StatusCode;
use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::cloud;
use crate::rest::{RestClient, flag, gauge, now_ms, text};

pub const DEFAULT_PORT: u16 = 443;
pub const DEFAULT_ADDRESS: &str = "eu.api.ovh.com";
const API: &str = "/1.0";
/// Serveurs décrits par famille (un état et deux listes de tâches chacun).
const MAX_SERVERS: usize = 30;
const PREFIX: &str = "ovh";

/// Les trois pièces de l'identifiant.
struct Keys {
    application: String,
    secret: String,
    consumer: String,
}

fn keys(credential: &Credential) -> Result<Keys, ProbeError> {
    match credential {
        Credential::UsernamePassword { username, password } => {
            let (secret, consumer) = password.split_once(':').ok_or_else(|| {
                ProbeError::Config(
                    "OVHcloud expects the password field as \"application secret:consumer key\" \
                     (the two joined by a colon)"
                        .to_string(),
                )
            })?;
            let (application, secret, consumer) = (username.trim(), secret.trim(), consumer.trim());
            if application.is_empty() || secret.is_empty() || consumer.is_empty() {
                return Err(ProbeError::Config(
                    "OVHcloud needs the application key, the application secret and the \
                     consumer key"
                        .to_string(),
                ));
            }
            Ok(Keys {
                application: application.to_string(),
                secret: secret.to_string(),
                consumer: consumer.to_string(),
            })
        }
        other => Err(ProbeError::Config(format!(
            "OVHcloud expects an application key and \"application secret:consumer key\", \
             configured: {other}"
        ))),
    }
}

/// Signature d'une requête : `$1$` + SHA-1 hexadécimal de
/// `secret+consommateur+méthode+URL+corps+horodatage`.
pub fn signature(
    secret: &str,
    consumer: &str,
    method: &str,
    url: &str,
    body: &str,
    timestamp: i64,
) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{secret}+{consumer}+{method}+{url}+{body}+{timestamp}"));
    let hex: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    format!("$1${hex}")
}

/// Une session : le client, les clés et l'horodatage du serveur.
struct Session<'a> {
    client: &'a RestClient,
    keys: &'a Keys,
    timestamp: i64,
}

impl Session<'_> {
    async fn get(&self, path: &str) -> Result<(StatusCode, String), ProbeError> {
        let full = format!("{API}{path}");
        let url = self.client.url(&full);
        let headers = [
            ("X-Ovh-Application", self.keys.application.clone()),
            ("X-Ovh-Consumer", self.keys.consumer.clone()),
            ("X-Ovh-Timestamp", self.timestamp.to_string()),
            (
                "X-Ovh-Signature",
                signature(&self.keys.secret, &self.keys.consumer, "GET", &url, "", self.timestamp),
            ),
        ];
        self.client.get_with(&full, &headers).await
    }

    /// Lit un document JSON. `None` : l'élément n'existe pas (ou plus), ou la
    /// clé de consommateur n'a pas le droit de l'appeler. Un identifiant ou une
    /// signature refusés sont une erreur d'authentification.
    async fn json(&self, path: &str) -> Result<Option<Value>, ProbeError> {
        let (status, body) = self.get(path).await?;
        if status.is_success() {
            return self.client.decode(&body, path).map(Some);
        }
        let lower = body.to_ascii_lowercase();
        match status.as_u16() {
            403 if lower.contains("not been granted") => Ok(None),
            404 => Ok(None),
            400 if ["signature", "application", "consumer", "credential"]
                .iter()
                .any(|word| lower.contains(word)) =>
            {
                Err(ProbeError::Auth(
                    "OVHcloud refused the signature (400): check the application key, the \
                     application secret and the consumer key, and the \"secret:consumer\" order \
                     of the password field."
                        .to_string(),
                ))
            }
            _ => Err(self.client.status_error(status, &body, path)),
        }
    }
}

/// L'heure du serveur d'OVHcloud, en secondes.
async fn server_time(client: &RestClient) -> Result<i64, ProbeError> {
    let path = format!("{API}/auth/time");
    let (status, body) = client.get(&path).await?;
    if !status.is_success() {
        return Err(client.status_error(status, &body, &path));
    }
    body.trim().parse().map_err(|_| {
        ProbeError::Protocol(format!("OVHcloud sent an unexpected time on {path}: {body}"))
    })
}

/// Où lire les tâches d'une famille (le nom du filtre diffère).
#[derive(Clone, Copy)]
struct TaskApi {
    segment: &'static str,
    param: &'static str,
}

const VPS_TASKS: TaskApi = TaskApi { segment: "tasks", param: "state" };
const DEDICATED_TASKS: TaskApi = TaskApi { segment: "task", param: "status" };

pub struct Server {
    pub name: String,
    pub detail: Value,
    /// Tâches en attente ou en cours ; `None` quand elles n'ont pas pu être lues.
    pub tasks: Option<usize>,
    /// `serviceInfos.expiration` (`AAAA-MM-JJ`) ; `None` quand elle n'est pas lisible.
    pub expiration: Option<String>,
}

#[derive(Default)]
pub struct Fleet {
    /// Serveurs de la liste, décrits ou non.
    pub listed: usize,
    pub servers: Vec<Server>,
}

/// Ce qu'une interrogation a lu ; `None` : famille non lisible avec cette clé.
#[derive(Default)]
pub struct Snapshot {
    pub vps: Option<Fleet>,
    pub dedicated: Option<Fleet>,
}

async fn active_tasks(
    session: &Session<'_>,
    base: &str,
    name: &str,
    api: TaskApi,
) -> Option<usize> {
    let mut total = 0;
    for state in ["todo", "doing"] {
        let path = format!("{base}/{name}/{}?{}={state}", api.segment, api.param);
        let list = session.json(&path).await.ok()??;
        total += list.as_array()?.len();
    }
    Some(total)
}

async fn server(
    session: &Session<'_>,
    base: &str,
    name: &str,
    api: TaskApi,
) -> Result<Option<Server>, ProbeError> {
    let Some(detail) = session.json(&format!("{base}/{name}")).await? else {
        return Ok(None);
    };
    let tasks = active_tasks(session, base, name, api).await;
    // Facultatif : une clé sans le droit sur `serviceInfos` ne fait pas échouer la sonde.
    let expiration = session
        .json(&format!("{base}/{name}/serviceInfos"))
        .await
        .ok()
        .flatten()
        .and_then(|info| text(&info, "expiration").map(str::to_string));
    Ok(Some(Server { name: name.to_string(), detail, tasks, expiration }))
}

async fn fleet(
    session: &Session<'_>,
    base: &str,
    api: TaskApi,
) -> Result<Option<Fleet>, ProbeError> {
    let Some(list) = session.json(base).await? else {
        return Ok(None);
    };
    let names: Vec<&str> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|name| !name.is_empty() && !name.contains(['/', '?', '#', ' ', '\\']))
        .collect();
    let fetched = futures::future::join_all(
        names.iter().take(MAX_SERVERS).map(|name| server(session, base, name, api)),
    )
    .await;
    let mut servers = Vec::new();
    for result in fetched {
        if let Some(found) = result? {
            servers.push(found);
        }
    }
    Ok(Some(Fleet { listed: names.len(), servers }))
}

async fn collect(target: &Target) -> Result<Snapshot, ProbeError> {
    let keys = keys(&target.credential)?;
    let client = RestClient::for_target(target, "https", DEFAULT_PORT, "OVHcloud")?;
    let timestamp = server_time(&client).await?;
    let session = Session { client: &client, keys: &keys, timestamp };
    let (vps, dedicated) = (
        fleet(&session, "/vps", VPS_TASKS).await?,
        fleet(&session, "/dedicated/server", DEDICATED_TASKS).await?,
    );
    if vps.is_none() && dedicated.is_none() {
        return Err(ProbeError::Auth(
            "OVHcloud granted this consumer key neither GET /vps* nor GET /dedicated/server*: \
             create it again with at least one of them."
                .to_string(),
        ));
    }
    Ok(Snapshot { vps, dedicated })
}

#[derive(Default)]
pub struct OvhCollector;

impl OvhCollector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Collector for OvhCollector {
    fn kind(&self) -> &'static str {
        "ovh"
    }

    async fn probe(&self, target: &Target) -> Result<Vec<Sample>, ProbeError> {
        let snapshot = collect(target).await?;
        Ok(samples(&snapshot, now_ms()))
    }

    async fn discover(&self, target: &Target) -> Result<Option<String>, ProbeError> {
        collect(target).await?;
        Ok(Some("ovh".to_string()))
    }
}

/// Un VPS fonctionne quand il tourne (ou fait une sauvegarde, sans s'arrêter).
fn vps_up(state: &str) -> bool {
    matches!(state, "running" | "backuping")
}

/// Un dédié va bien quand la supervision d'OVHcloud le dit `ok` et qu'il est allumé.
fn dedicated_up(state: &str, power: Option<&str>) -> bool {
    state == "ok" && power != Some("poweroff")
}

/// Secondes restantes avant la date `AAAA-MM-JJ` d'OVHcloud (minuit UTC ; négatif : échue).
fn seconds_until(expiration: &str, ts_ms: i64) -> Option<f64> {
    let date = chrono::NaiveDate::parse_from_str(expiration.get(..10)?, "%Y-%m-%d").ok()?;
    let end = date.and_hms_opt(0, 0, 0)?.and_utc().timestamp();
    Some((end - ts_ms / 1000) as f64)
}

/// L'échéance d'un service (`ovh_service_expiry_seconds`), lue par `GET /api/deadlines`.
fn expiry(server: &Server, family: &str, ts_ms: i64) -> Option<Sample> {
    let left = seconds_until(server.expiration.as_deref()?, ts_ms)?;
    Some(
        gauge(&format!("{PREFIX}_service_expiry_seconds"), left, ts_ms)
            .with_label("service", server.name.as_str())
            .with_label("family", family),
    )
}

pub fn samples(snapshot: &Snapshot, ts_ms: i64) -> Vec<Sample> {
    let g = |name: &str, value: f64| gauge(&format!("{PREFIX}_{name}"), value, ts_ms);
    let mut out = Vec::new();
    let (mut listed, mut down, mut tasks_active, mut tasks_readable) = (0usize, 0usize, 0, true);

    if let Some(fleet) = &snapshot.vps {
        listed += fleet.listed;
        for server in &fleet.servers {
            let state = text(&server.detail, "state").unwrap_or("unknown");
            let up = vps_up(state);
            down += usize::from(!up);
            let model = server.detail.get("model").and_then(|m| text(m, "name")).unwrap_or("");
            let named = |sample: Sample| sample.with_label("vps", server.name.as_str());
            out.push(named(flag(&format!("{PREFIX}_vps_up"), up, ts_ms)));
            out.push(
                named(g("vps_info", 1.0))
                    .with_label("state", state)
                    .with_label("zone", text(&server.detail, "zone").unwrap_or(""))
                    .with_label("model", model),
            );
            match server.tasks {
                Some(count) => {
                    tasks_active += count;
                    out.push(named(g("vps_tasks_active", count as f64)));
                }
                None => tasks_readable = false,
            }
            let shown = text(&server.detail, "displayName").unwrap_or(&server.name);
            out.push(cloud::item(PREFIX, "VPS", shown, up, state, ts_ms));
            out.extend(expiry(server, "vps", ts_ms));
        }
    }
    if let Some(fleet) = &snapshot.dedicated {
        listed += fleet.listed;
        for server in &fleet.servers {
            let state = text(&server.detail, "state").unwrap_or("unknown");
            let power = text(&server.detail, "powerState");
            let up = dedicated_up(state, power);
            down += usize::from(!up);
            let named = |sample: Sample| sample.with_label("server", server.name.as_str());
            out.push(named(flag(&format!("{PREFIX}_dedicated_up"), up, ts_ms)));
            if let Some(monitored) = server.detail.get("monitoring").and_then(Value::as_bool) {
                out.push(named(flag(&format!("{PREFIX}_dedicated_monitoring"), monitored, ts_ms)));
            }
            out.push(
                named(g("dedicated_info", 1.0))
                    .with_label("state", state)
                    .with_label("power", power.unwrap_or(""))
                    .with_label("datacenter", text(&server.detail, "datacenter").unwrap_or("")),
            );
            match server.tasks {
                Some(count) => {
                    tasks_active += count;
                    out.push(named(g("dedicated_tasks_active", count as f64)));
                }
                None => tasks_readable = false,
            }
            let detail = if state != "ok" {
                state
            } else if power == Some("poweroff") {
                "poweroff"
            } else {
                "ok"
            };
            out.push(cloud::item(PREFIX, "Dedicated servers", &server.name, up, detail, ts_ms));
            out.extend(expiry(server, "dedicated", ts_ms));
        }
    }

    out.push(g("servers", listed as f64));
    out.push(g("servers_down", down as f64));
    out.push(g("tasks_active", tasks_active as f64));
    out.push(flag(&format!("{PREFIX}_vps_readable"), snapshot.vps.is_some(), ts_ms));
    out.push(flag(&format!("{PREFIX}_dedicated_readable"), snapshot.dedicated.is_some(), ts_ms));
    out.push(flag(&format!("{PREFIX}_tasks_readable"), tasks_readable, ts_ms));
    out.push(cloud::figure(PREFIX, "Servers", "", listed as f64, ts_ms));
    out.push(cloud::figure(PREFIX, "Servers down", "", down as f64, ts_ms));
    out.push(cloud::figure(PREFIX, "Tasks in progress", "", tasks_active as f64, ts_ms));
    out
}

#[cfg(test)]
mod tests;
