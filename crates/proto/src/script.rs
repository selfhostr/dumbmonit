//! Contrôles personnalisés exécutés par l'agent : un script de l'administrateur
//! ou n'importe quel greffon Nagios (`check_disk`, `check_http`…).
//!
//! # Qui décide de quoi
//!
//! Le serveur décrit le contrôle (programme, arguments, environnement, délai,
//! format de sortie) dans l'accusé de réception ([`crate::PushAck::script_checks`]) ;
//! l'agent l'exécute et renvoie le verdict au lot suivant
//! ([`crate::PushBatch::script_results`]). Mais c'est **la machine** qui dit ce
//! qui peut tourner : l'agent refuse tout tant que `scripts: true` n'est pas
//! posé dans son agent.yaml, et ne lance que des exécutables rangés dans les
//! dossiers que ce fichier autorise (`crates/agent/src/scripts/allow.rs`). Un
//! serveur détourné ne peut donc rien exécuter que l'administrateur de la
//! machine n'ait déposé lui-même.
//!
//! Aucun interpréteur n'est jamais invoqué : le programme et ses arguments vont
//! tels quels au système (`argv`). Les règles ci-dessous sont appliquées des deux
//! côtés : le serveur refuse à l'enregistrement ce que l'agent refuserait à
//! l'exécution.
//!
//! # Compatibilité
//!
//! Les deux champs sont facultatifs : un agent antérieur ignore les contrôles,
//! un serveur antérieur ignore les résultats. Les valeurs ouvertes (format,
//! nature d'une erreur) voyagent en texte et le statut en entier, pour qu'une
//! valeur inconnue d'une version plus récente ne fasse jamais rejeter tout un
//! accusé ou tout un lot.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

mod parse;

pub use parse::{
    ParsedOutput, parse_json, parse_nagios, parse_output, parse_perfdata, parse_prometheus,
};

/// Type des cibles « contrôle personnalisé exécuté par un agent ».
pub const SCRIPT_CHECK_KIND: &str = "agent_script";

/// Délai d'exécution par défaut, en secondes.
pub const CHECK_DEFAULT_TIMEOUT_SECS: u64 = 10;
/// Délai d'exécution maximal, en secondes.
pub const CHECK_MAX_TIMEOUT_SECS: u64 = 120;
/// Période minimale entre deux exécutions d'un même contrôle.
pub const CHECK_MIN_INTERVAL_SECS: u64 = 10;
/// Période maximale : une journée.
pub const CHECK_MAX_INTERVAL_SECS: u64 = 86_400;
/// Nombre maximal de contrôles confiés à un agent.
pub const MAX_SCRIPT_CHECKS: usize = 64;
/// Longueur maximale du programme, en octets.
pub const MAX_COMMAND_LEN: usize = 512;
/// Nombre maximal d'arguments.
pub const MAX_CHECK_ARGS: usize = 32;
/// Longueur maximale d'un argument ou d'une valeur d'environnement, en octets.
pub const MAX_CHECK_ARG_LEN: usize = 1024;
/// Nombre maximal de variables d'environnement propres au contrôle.
pub const MAX_CHECK_ENV: usize = 16;
/// Octets de sortie standard lus au plus ; le reste est lu et jeté.
pub const CHECK_OUTPUT_MAX_BYTES: usize = 64 * 1024;
/// Mesures retenues au plus par exécution.
pub const MAX_CHECK_METRICS: usize = 64;
/// Longueur maximale du message rendu, en caractères.
pub const MAX_CHECK_MESSAGE: usize = 1024;
/// Longueur maximale du nom d'une mesure, en caractères.
pub const MAX_METRIC_NAME: usize = 200;

/// Codes de statut, ceux des greffons Nagios.
pub const STATUS_OK: u8 = 0;
pub const STATUS_WARNING: u8 = 1;
pub const STATUS_CRITICAL: u8 = 2;
pub const STATUS_UNKNOWN: u8 = 3;

/// Nature d'un contrôle qui n'a pas pu rendre de verdict.
///
/// - [`ERROR_REFUSED`] : l'agent a refusé de l'exécuter (fonction désactivée,
///   programme hors des dossiers autorisés, réglage invalide) — une erreur de
///   configuration, affichée mais jamais notifiée ;
/// - [`ERROR_TIMEOUT`] : tué après son délai ;
/// - [`ERROR_FAILED`] : impossible à lancer (fichier disparu, droits).
pub const ERROR_REFUSED: &str = "refused";
pub const ERROR_TIMEOUT: &str = "timeout";
pub const ERROR_FAILED: &str = "failed";

/// Marques remplacées dans les arguments par les identifiants du moniteur,
/// au moment de l'exécution, sur l'agent.
pub const SECRET_KEYS: &[&str] = &["token", "username", "password"];

/// Formats de sortie compris.
pub const FORMAT_NAGIOS: &str = "nagios";
pub const FORMAT_JSON: &str = "json";
pub const FORMAT_PROMETHEUS: &str = "prometheus";
pub const FORMATS: &[&str] = &[FORMAT_NAGIOS, FORMAT_JSON, FORMAT_PROMETHEUS];

/// Variables qu'un contrôle ne peut pas poser : elles font charger du code
/// d'ailleurs (`LD_PRELOAD`, `BASH_ENV`, `PYTHONPATH`…) ou changent ce que le
/// programme exécute (`PATH`, `IFS`, `COMSPEC`).
const DENIED_ENV: &[&str] = &[
    "PATH",
    "IFS",
    "ENV",
    "BASH_ENV",
    "SHELLOPTS",
    "BASHOPTS",
    "PS4",
    "CDPATH",
    "GLOBIGNORE",
    "PERLLIB",
    "PERLIO",
    "RUBYOPT",
    "RUBYLIB",
    "NODE_OPTIONS",
    "NODE_PATH",
    "GCONV_PATH",
    "HOSTALIASES",
    "LOCALDOMAIN",
    "RES_OPTIONS",
    "NLSPATH",
    "GLIBC_TUNABLES",
    "COMSPEC",
    "PATHEXT",
    "SYSTEMROOT",
    "WINDIR",
    "PSMODULEPATH",
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "PHPRC",
    "PHP_INI_SCAN_DIR",
];

/// Préfixes refusés pour la même raison ; `DUMBMONIT_` est réservé à l'agent.
const DENIED_ENV_PREFIXES: &[&str] =
    &["LD_", "DYLD_", "MALLOC_", "BASH_FUNC_", "PYTHON", "PERL5", "LUA_", "DUMBMONIT_"];

/// Un contrôle confié à un agent.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptCheck {
    /// Identifiant de la cible `agent_script` : c'est sous elle que le
    /// résultat sera rangé.
    pub id: i64,
    /// Nom d'un exécutable d'un dossier autorisé (`check_disk`), ou son chemin
    /// absolu.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub timeout_secs: u64,
    pub interval_secs: u64,
    /// [`FORMAT_NAGIOS`], [`FORMAT_JSON`] ou [`FORMAT_PROMETHEUS`]. Un format
    /// inconnu de l'agent rend un résultat `refused`, jamais une erreur de
    /// lecture de l'accusé.
    #[serde(default = "default_format")]
    pub format: String,
    /// Identifiants du moniteur (`token`, `username`, `password`), remplacés
    /// dans les arguments (`{{secret.password}}`) et passés en variables
    /// `DUMBMONIT_SECRET_*`. Jamais journalisés.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, String>,
}

fn default_format() -> String {
    FORMAT_NAGIOS.to_string()
}

/// Un `Debug` dérivé finirait dans une trace : les secrets n'y figurent pas.
impl fmt::Debug for ScriptCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptCheck")
            .field("id", &self.id)
            .field("command", &self.command)
            .field("args", &self.args.len())
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .field("timeout_secs", &self.timeout_secs)
            .field("interval_secs", &self.interval_secs)
            .field("format", &self.format)
            .field("secrets", &self.secrets.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ScriptCheck {
    /// Vérifie le contrôle entier : ce que l'agent exécuterait tel quel.
    pub fn validate(&self) -> Result<(), String> {
        validate_command(&self.command)?;
        validate_args(&self.args)?;
        validate_env(&self.env)?;
        validate_secrets(&self.secrets)?;
        if !(1..=CHECK_MAX_TIMEOUT_SECS).contains(&self.timeout_secs) {
            return Err(format!("the timeout must be between 1 and {CHECK_MAX_TIMEOUT_SECS} s"));
        }
        if !FORMATS.contains(&self.format.as_str()) {
            return Err(format!("unknown output format '{}'", printable(&self.format)));
        }
        Ok(())
    }

    /// Période effective, bornée.
    pub fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.interval_secs.clamp(CHECK_MIN_INTERVAL_SECS, CHECK_MAX_INTERVAL_SECS),
        )
    }

    /// Délai effectif, borné.
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout_secs.clamp(1, CHECK_MAX_TIMEOUT_SECS))
    }

    /// Les arguments, marques `{{secret.…}}` remplacées.
    pub fn expanded_args(&self) -> Vec<String> {
        self.args
            .iter()
            .map(|arg| {
                let mut arg = arg.clone();
                for key in SECRET_KEYS {
                    let mark = format!("{{{{secret.{key}}}}}");
                    if arg.contains(&mark) {
                        let value = self.secrets.get(*key).map(String::as_str).unwrap_or("");
                        arg = arg.replace(&mark, value);
                    }
                }
                arg
            })
            .collect()
    }
}

/// Le verdict d'une exécution, tel que l'agent le rend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptResult {
    /// Identifiant du contrôle ([`ScriptCheck::id`]).
    pub id: i64,
    /// Début de l'exécution, en millisecondes Unix.
    pub ran_at_ms: i64,
    #[serde(default)]
    pub duration_ms: u64,
    /// [`STATUS_OK`] à [`STATUS_UNKNOWN`] ; toute autre valeur vaut UNKNOWN.
    pub status: u8,
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<CheckMetric>,
    /// Présent quand le contrôle n'a pas rendu de verdict : [`ERROR_REFUSED`],
    /// [`ERROR_TIMEOUT`] ou [`ERROR_FAILED`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ScriptResult {
    /// Un contrôle sans verdict, avec sa raison.
    pub fn failure(id: i64, ran_at_ms: i64, kind: &str, message: impl Into<String>) -> Self {
        Self {
            id,
            ran_at_ms,
            duration_ms: 0,
            status: STATUS_UNKNOWN,
            message: truncate_chars(&message.into(), MAX_CHECK_MESSAGE),
            exit_code: None,
            metrics: Vec::new(),
            error: Some(kind.to_string()),
        }
    }

    /// Borne et assainit un résultat reçu : un agent modifié ne doit pas
    /// pouvoir faire écrire des mégaoctets ni des valeurs non finies.
    pub fn sanitized(mut self) -> Self {
        if self.status > STATUS_UNKNOWN {
            self.status = STATUS_UNKNOWN;
        }
        self.message = truncate_chars(&strip_controls(&self.message), MAX_CHECK_MESSAGE);
        self.metrics.retain(|metric| {
            metric.value.is_finite()
                && !metric.name.trim().is_empty()
                && metric.name.chars().count() <= MAX_METRIC_NAME
                && !metric.name.chars().any(char::is_control)
        });
        self.metrics.truncate(MAX_CHECK_METRICS);
        for metric in &mut self.metrics {
            metric.unit = truncate_chars(&strip_controls(&metric.unit), 16);
            for bound in [&mut metric.warn, &mut metric.crit, &mut metric.min, &mut metric.max] {
                *bound = bound.filter(|value| value.is_finite());
            }
        }
        if let Some(kind) = &self.error
            && ![ERROR_REFUSED, ERROR_TIMEOUT, ERROR_FAILED].contains(&kind.as_str())
        {
            self.error = Some(ERROR_FAILED.to_string());
        }
        self
    }
}

/// Une mesure rendue par un contrôle : une donnée de performance Nagios, une
/// entrée `metrics` du format JSON, une série Prometheus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckMetric {
    pub name: String,
    pub value: f64,
    /// Unité normalisée : `s`, `B`, `%`, ou vide.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    /// Compteur (`c` de Nagios, `# TYPE … counter` de Prometheus).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub counter: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

impl CheckMetric {
    pub fn gauge(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value,
            unit: String::new(),
            counter: false,
            warn: None,
            crit: None,
            min: None,
            max: None,
        }
    }
}

/// Le statut correspondant au code de sortie d'un greffon : 0 à 3 tels quels,
/// tout le reste (code inconnu, processus tué par un signal) vaut UNKNOWN.
pub fn status_from_exit(code: Option<i32>) -> u8 {
    match code {
        Some(code @ 0..=3) => code as u8,
        _ => STATUS_UNKNOWN,
    }
}

/// Programme d'un contrôle : un nom de fichier seul (`check_disk`,
/// `backup-age.sh`), cherché dans les dossiers autorisés de l'agent, ou un
/// chemin absolu (`/usr/lib/nagios/plugins/check_disk`,
/// `C:\ProgramData\DumbMonit\scripts\check.ps1`).
///
/// Refusés : un chemin relatif à plusieurs éléments, toute composante `..`, un
/// chemin réseau (`\\serveur\partage`), un flux NTFS (`fichier:flux`), et tout
/// caractère de contrôle. L'agent, lui, vérifie ensuite que le fichier réel est
/// bien dans un dossier autorisé.
pub fn validate_command(command: &str) -> Result<(), String> {
    let shown = printable(command);
    if command.trim().is_empty() {
        return Err("the command is empty".to_string());
    }
    if command.len() > MAX_COMMAND_LEN {
        return Err(format!("the command is longer than {MAX_COMMAND_LEN} bytes"));
    }
    if command.chars().any(char::is_control) || command != command.trim() {
        return Err(format!("the command '{shown}' has control characters or spaces at its ends"));
    }
    if command.starts_with("\\\\") || command.starts_with("//") {
        return Err(format!("'{shown}': network paths are not allowed"));
    }
    let bytes = command.as_bytes();
    let windows_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let absolute = command.starts_with('/') || windows_absolute;
    let colon_ok = command.char_indices().all(|(i, c)| c != ':' || (windows_absolute && i == 1));
    if !colon_ok {
        return Err(format!("'{shown}': ':' is only allowed after a drive letter"));
    }
    let parts: Vec<&str> = command.split(['/', '\\']).collect();
    if parts.contains(&"..") {
        return Err(format!("'{shown}': '..' is not allowed"));
    }
    if !absolute && parts.len() > 1 {
        return Err(format!(
            "'{shown}': give a file name of an allowed directory or an absolute path"
        ));
    }
    if !absolute && (command == "." || command.starts_with('-')) {
        return Err(format!("'{shown}' is not a file name"));
    }
    Ok(())
}

/// Arguments : 32 au plus, 1 Kio chacun, sans octet nul ni saut de ligne.
/// Ils ne passent par aucun interpréteur : `;`, `$()` ou `|` y restent du texte.
pub fn validate_args(args: &[String]) -> Result<(), String> {
    if args.len() > MAX_CHECK_ARGS {
        return Err(format!("too many arguments (max {MAX_CHECK_ARGS})"));
    }
    for arg in args {
        if arg.len() > MAX_CHECK_ARG_LEN {
            return Err(format!("an argument is longer than {MAX_CHECK_ARG_LEN} bytes"));
        }
        if arg.chars().any(|c| c == '\0' || c == '\n' || c == '\r') {
            let shown = printable(arg);
            return Err(format!("the argument '{shown}' contains a NUL byte or a line break"));
        }
    }
    Ok(())
}

/// Vrai si un contrôle peut poser cette variable d'environnement.
pub fn env_name_allowed(name: &str) -> bool {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !first_ok || !rest_ok || name.len() > 64 {
        return false;
    }
    let upper = name.to_ascii_uppercase();
    !DENIED_ENV.contains(&upper.as_str())
        && !DENIED_ENV_PREFIXES.iter().any(|prefix| upper.starts_with(prefix))
}

/// Environnement propre au contrôle : 16 variables au plus, noms
/// `[A-Za-z_][A-Za-z0-9_]*`, aucune de celles qui font charger du code.
pub fn validate_env(env: &BTreeMap<String, String>) -> Result<(), String> {
    if env.len() > MAX_CHECK_ENV {
        return Err(format!("too many environment variables (max {MAX_CHECK_ENV})"));
    }
    for (name, value) in env {
        if !env_name_allowed(name) {
            return Err(format!("the environment variable '{}' is not allowed", printable(name)));
        }
        if value.len() > MAX_CHECK_ARG_LEN || value.contains('\0') {
            return Err(format!("the value of '{name}' is too long or contains a NUL byte"));
        }
    }
    Ok(())
}

fn validate_secrets(secrets: &BTreeMap<String, String>) -> Result<(), String> {
    for (key, value) in secrets {
        if !SECRET_KEYS.contains(&key.as_str()) {
            return Err(format!("unknown secret '{}'", printable(key)));
        }
        if value.len() > MAX_CHECK_ARG_LEN || value.contains('\0') {
            return Err(format!("the secret '{key}' is too long or contains a NUL byte"));
        }
    }
    Ok(())
}

/// Une chaîne reçue, rendue sûre à afficher dans un message d'erreur.
fn printable(text: &str) -> String {
    text.chars().take(64).map(|c| if c.is_control() { '?' } else { c }).collect()
}

/// Les caractères de contrôle remplacés par des espaces (sauf rien d'autre).
pub(crate) fn strip_controls(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// Les `max` premiers caractères.
pub(crate) fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests;
