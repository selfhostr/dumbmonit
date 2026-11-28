//! Contrôles personnalisés : un script de l'administrateur ou un greffon
//! Nagios, exécuté sur la machine à la demande du serveur.
//!
//! Le serveur confie les contrôles dans l'accusé de réception
//! ([`dumbmonit_proto::PushAck::script_checks`]) ; [`ScriptRunner`] les
//! exécute chacun à sa période, en tâche de fond, et garde le dernier verdict
//! de chacun jusqu'à ce qu'un lot accepté l'ait emporté
//! ([`dumbmonit_proto::PushBatch::script_results`]).
//!
//! # Modèle de menace
//!
//! Le serveur choisit le programme, ses arguments et une partie de son
//! environnement : un serveur compromis doit pourtant rester incapable de faire
//! exécuter une commande arbitraire. D'où :
//!
//! - rien ne tourne sans `scripts: true` dans agent.yaml — un fichier que seul
//!   l'administrateur de la machine écrit ;
//! - seuls les exécutables posés directement dans `scripts_dirs` sont lancés,
//!   liens résolus, droits vérifiés, shells et interpréteurs refusés
//!   ([`allow`]) ;
//! - aucun shell : `argv` tel quel, variables qui font charger du code
//!   refusées, sortie et durée bornées, groupe de processus tué au délai
//!   ([`exec`]) ;
//! - `scripts_user` fait tourner les contrôles sous un compte sans droits
//!   quand l'agent est root.
//!
//! Ce qui reste possible à un serveur compromis : lancer, avec les arguments
//! de son choix, les programmes que l'administrateur a autorisés. Les greffons
//! Nagios connus pour exécuter une autre commande ou écrire un fichier sont
//! refusés, mais ouvrir tout `/usr/lib/nagios/plugins` reste un choix à faire
//! en connaissance de cause (voir `docs/devices/custom-scripts.md`).

pub mod allow;
pub mod exec;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dumbmonit_proto::script::{
    ERROR_FAILED, ERROR_REFUSED, ERROR_TIMEOUT, MAX_SCRIPT_CHECKS, parse_output,
};
use dumbmonit_proto::{ScriptCheck, ScriptResult};
use tracing::{debug, info, warn};

/// Contrôles exécutés en même temps, au plus.
pub const DEFAULT_MAX_CONCURRENT: usize = 4;

/// Pas de la boucle qui lance les contrôles arrivés à échéance.
const TICK: Duration = Duration::from_secs(1);

/// Message rendu quand la fonction n'est pas activée sur la machine.
pub const DISABLED: &str = "custom checks are disabled on this machine: add \"scripts: true\" \
     to agent.yaml (and the directories in scripts_dirs), then restart the agent";

/// Réglages locaux des contrôles personnalisés.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptsConfig {
    /// `scripts: true` : sans lui, aucun contrôle ne tourne.
    pub enabled: bool,
    /// Dossiers dont les exécutables peuvent être lancés.
    pub dirs: Vec<PathBuf>,
    /// Compte sous lequel tournent les contrôles (Unix) : un nom ou `uid:gid`.
    /// `None` : celui de l'agent.
    pub user: Option<String>,
    pub max_concurrent: usize,
}

impl Default for ScriptsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dirs: default_dirs(None),
            user: None,
            max_concurrent: DEFAULT_MAX_CONCURRENT,
        }
    }
}

/// Dossier par défaut, à côté de agent.yaml : `/etc/dumbmonit/scripts.d`,
/// `C:\ProgramData\DumbMonit\scripts`.
pub fn default_dirs(config_path: Option<&Path>) -> Vec<PathBuf> {
    let base = config_path
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .or_else(|| crate::config::Config::default_path().parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("/etc/dumbmonit"));
    vec![base.join(if cfg!(windows) { "scripts" } else { "scripts.d" })]
}

/// Un contrôle planifié.
#[derive(Debug)]
struct Planned {
    check: ScriptCheck,
    next_due: Instant,
    running: bool,
}

#[derive(Debug, Default)]
struct Shared {
    checks: BTreeMap<i64, Planned>,
    /// Dernier verdict de chaque contrôle, pas encore accepté par le serveur.
    results: BTreeMap<i64, ScriptResult>,
}

/// Exécute les contrôles confiés par le serveur.
pub struct ScriptRunner {
    config: Arc<ScriptsConfig>,
    shared: Arc<Mutex<Shared>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl ScriptRunner {
    pub fn new(config: ScriptsConfig) -> Self {
        if config.enabled {
            let dirs: Vec<String> = config.dirs.iter().map(|d| d.display().to_string()).collect();
            info!(dirs = dirs.join(", "), "custom checks enabled");
        }
        Self { config: Arc::new(config), shared: Arc::default(), task: None }
    }

    /// Remplace les contrôles par ceux du dernier accusé de réception. Un
    /// contrôle inchangé garde son échéance ; un nouveau ou un modifié tourne
    /// au prochain pas. La tâche de fond naît au premier contrôle.
    pub fn set_checks(&mut self, mut checks: Vec<ScriptCheck>) {
        checks.truncate(MAX_SCRIPT_CHECKS);
        let now = Instant::now();
        let wanted = match self.shared.lock() {
            Ok(mut guard) => {
                let shared = &mut *guard;
                let mut next = BTreeMap::new();
                for check in checks {
                    let planned = match shared.checks.remove(&check.id) {
                        Some(old) if old.check == check => old,
                        Some(old) => Planned { check, next_due: now, running: old.running },
                        None => Planned { check, next_due: now, running: false },
                    };
                    next.insert(planned.check.id, planned);
                }
                shared.checks = next;
                let checks = &shared.checks;
                shared.results.retain(|id, _| checks.contains_key(id));
                !shared.checks.is_empty()
            }
            Err(_) => false,
        };
        if wanted && self.task.is_none() {
            let config = self.config.clone();
            let shared = self.shared.clone();
            self.task = Some(tokio::spawn(schedule(config, shared)));
        }
    }

    /// Les verdicts à joindre au prochain lot, `None` s'il n'y en a aucun.
    pub fn pending(&self) -> Option<Vec<ScriptResult>> {
        let shared = self.shared.lock().ok()?;
        (!shared.results.is_empty()).then(|| shared.results.values().cloned().collect())
    }

    /// Oublie les verdicts qu'un lot accepté a emportés — sauf ceux qu'une
    /// exécution plus récente a remplacés entre-temps.
    pub fn acknowledge(&self, sent: Option<&[ScriptResult]>) {
        let (Some(sent), Ok(mut shared)) = (sent, self.shared.lock()) else { return };
        for result in sent {
            let kept = shared.results.get(&result.id).map(|kept| kept.ran_at_ms);
            if kept == Some(result.ran_at_ms) {
                shared.results.remove(&result.id);
            }
        }
    }
}

impl Drop for ScriptRunner {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Boucle de fond : à chaque pas, lance les contrôles arrivés à échéance, au
/// plus `max_concurrent` à la fois.
async fn schedule(config: Arc<ScriptsConfig>, shared: Arc<Mutex<Shared>>) {
    let permits = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent.max(1)));
    let user = resolve_user(&config);
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let now = Instant::now();
        let due: Vec<ScriptCheck> = match shared.lock() {
            Ok(mut shared) => shared
                .checks
                .values_mut()
                .filter(|planned| !planned.running && planned.next_due <= now)
                .map(|planned| {
                    planned.running = true;
                    planned.next_due = now + planned.check.interval();
                    planned.check.clone()
                })
                .collect(),
            Err(_) => return,
        };
        for check in due {
            let (config, shared, permits, user) =
                (config.clone(), shared.clone(), permits.clone(), user.clone());
            tokio::spawn(async move {
                let Ok(_permit) = permits.acquire_owned().await else { return };
                let result = run_check(&config, &user, &check).await;
                let Ok(mut guard) = shared.lock() else { return };
                let shared = &mut *guard;
                // Retiré ou modifié pendant l'exécution : ce verdict ne vaut
                // plus pour le contrôle actuel.
                if let Some(planned) = shared.checks.get_mut(&check.id) {
                    planned.running = false;
                    if planned.check == check {
                        shared.results.insert(check.id, result);
                    }
                }
            });
        }
    }
}

/// Le compte des contrôles, lu une fois : `Err` garde la raison, rendue à
/// chaque contrôle tant que agent.yaml n'est pas corrigé.
#[cfg(unix)]
fn resolve_user(config: &ScriptsConfig) -> Result<Option<(u32, u32)>, String> {
    match config.user.as_deref() {
        None => Ok(None),
        Some(user) => allow::lookup_user(user).map(Some),
    }
}

#[cfg(not(unix))]
fn resolve_user(config: &ScriptsConfig) -> Result<Option<(u32, u32)>, String> {
    if let Some(user) = config.user.as_deref() {
        warn!(user, "scripts_user is only supported on Unix, ignored");
    }
    Ok(None)
}

/// Exécute un contrôle et rend son verdict. Ne panique jamais : un refus ou
/// un échec devient un résultat qui dit pourquoi.
pub async fn run_check(
    config: &ScriptsConfig,
    user: &Result<Option<(u32, u32)>, String>,
    check: &ScriptCheck,
) -> ScriptResult {
    let ran_at_ms = chrono::Utc::now().timestamp_millis();
    let started = Instant::now();
    let refuse =
        |message: String| ScriptResult::failure(check.id, ran_at_ms, ERROR_REFUSED, message);
    if !config.enabled {
        return refuse(DISABLED.to_string());
    }
    if let Err(error) = check.validate() {
        return refuse(error);
    }
    let user = match user {
        Ok(user) => *user,
        Err(error) => return refuse(error.clone()),
    };
    let program = match allow::resolve(&config.dirs, &check.command) {
        Ok(program) => program,
        Err(error) => {
            warn!(check = check.id, command = check.command, %error, "custom check refused");
            return refuse(error);
        }
    };
    let env = check_env(check);
    let args = check.expanded_args();
    let ended = exec::run(&program, &args, &env, check.timeout(), user).await;
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut result = match ended {
        Ok(exec::Ended::Finished { code, stdout, stderr }) => {
            let Some(parsed) = parse_output(&check.format, code, &stdout, &stderr) else {
                return refuse(format!("unknown output format '{}'", check.format));
            };
            ScriptResult {
                id: check.id,
                ran_at_ms,
                duration_ms,
                status: parsed.status,
                message: parsed.message,
                exit_code: code,
                metrics: parsed.metrics,
                error: None,
            }
        }
        Ok(exec::Ended::TimedOut) => ScriptResult::failure(
            check.id,
            ran_at_ms,
            ERROR_TIMEOUT,
            format!("killed: still running after {} s", check.timeout().as_secs()),
        ),
        Err(error) => ScriptResult::failure(check.id, ran_at_ms, ERROR_FAILED, error),
    };
    result.duration_ms = duration_ms;
    debug!(check = check.id, status = result.status, duration_ms, "custom check run");
    result
}

/// L'environnement propre au contrôle : le sien, son identifiant, et les
/// identifiants du moniteur en `DUMBMONIT_SECRET_*`.
fn check_env(check: &ScriptCheck) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> =
        check.env.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
    env.push(("DUMBMONIT_CHECK_ID".to_string(), check.id.to_string()));
    for (key, value) in &check.secrets {
        env.push((format!("DUMBMONIT_SECRET_{}", key.to_ascii_uppercase()), value.clone()));
    }
    env
}

#[cfg(test)]
mod tests;
