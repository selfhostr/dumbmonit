//! Copies hors site des sauvegardes locales.
//!
//! Une sauvegarde locale protège de la bêtise du soir, pas d'un disque perdu.
//! Après chaque sauvegarde locale réussie (planifiée ou « Sauvegarder
//! maintenant »), le même fichier part vers chaque destination active : un
//! seau S3 ou compatible, un dossier WebDAV, un dossier SFTP. Chaque
//! destination garde ses `keep` copies les plus récentes ; seuls les noms que
//! nous écrivons (`dumbmonit-AAAAMMJJ-HHMMSS.db` et sa `.key`) sont jamais
//! supprimés là-bas.
//!
//! Règles qui ne se discutent pas :
//!
//! * un échec d'envoi ne touche jamais à la sauvegarde locale : il est
//!   consigné sur la destination, exposé en métrique, et c'est tout ;
//! * `secret.key` ne part que si l'administrateur l'a demandé pour cette
//!   destination (`include_key`) : sans lui, une copie volée là-bas ne
//!   déchiffre aucun identifiant d'équipement ;
//! * les secrets des destinations sont chiffrés par la clé d'instance et ne
//!   ressortent jamais de l'API (`has_secret`) ;
//! * une clé d'hôte SFTP inconnue ou changée fait refuser la connexion avant
//!   toute authentification : l'administrateur confirme l'empreinte.

pub mod bundle;
mod run;
pub mod s3;
pub mod sftp;
pub mod sftp_proto;
pub mod sigv4;
pub mod store;
pub mod transfer;
pub mod webdav;

pub use run::{
    Retry, TestOutcome, after_local_backup, plan_prune, push_all, test_destination, uploading,
};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::backup::local;
use crate::untrusted;
use transfer::Failure;

/// Secrets d'une destination, par champ (`secret_access_key`, `password`,
/// `private_key`, `key_passphrase`).
pub type Secrets = BTreeMap<String, String>;

pub const MAX_NAME_LEN: usize = 64;
pub const MAX_KEEP: u32 = 365;
pub const DEFAULT_KEEP: u32 = 7;
/// Longueur maximale d'un secret ; une clé privée tient largement dedans.
const MAX_SECRET_LEN: usize = 16 * 1024;
const MAX_PATH_LEN: usize = 512;
const FINGERPRINT_HELP: &str =
    "The host key fingerprint must look like SHA256:… (as ssh-keygen -lf prints it).";

/// Les trois sortes de destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    S3,
    Webdav,
    Sftp,
}

impl Kind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "s3" => Some(Self::S3),
            "webdav" => Some(Self::Webdav),
            "sftp" => Some(Self::Sftp),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::S3 => "s3",
            Self::Webdav => "webdav",
            Self::Sftp => "sftp",
        }
    }

    /// Les champs secrets que cette sorte accepte.
    pub fn secret_fields(self) -> &'static [&'static str] {
        match self {
            Self::S3 => &["secret_access_key"],
            Self::Webdav => &["password"],
            Self::Sftp => &["password", "private_key", "key_passphrase"],
        }
    }
}

/// S3 ou compatible. `endpoint` vide : AWS, `https://s3.<région>.amazonaws.com`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S3Settings {
    #[serde(default)]
    pub endpoint: String,
    /// Vide : `us-east-1`. Cloudflare R2 attend `auto`.
    #[serde(default)]
    pub region: String,
    pub bucket: String,
    /// Dossier dans le seau, sans `/` au début ni à la fin ; vide : la racine.
    #[serde(default)]
    pub prefix: String,
    pub access_key_id: String,
    /// `https://hôte/seau/objet` plutôt que `https://seau.hôte/objet` :
    /// MinIO et la plupart des serveurs auto-hébergés le demandent.
    #[serde(default)]
    pub path_style: bool,
}

/// Dossier WebDAV (Nextcloud, ownCloud, un serveur Apache ou nginx…).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebDavSettings {
    /// URL du dossier, qui doit exister.
    pub url: String,
    #[serde(default)]
    pub username: String,
}

/// Dossier sur un serveur SFTP.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SftpSettings {
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub username: String,
    /// Dossier distant ; vide : le dossier de connexion du compte.
    #[serde(default)]
    pub directory: String,
    /// Empreinte épinglée de la clé d'hôte (`SHA256:…`). Absente : aucune
    /// connexion n'est authentifiée tant que l'administrateur ne l'a pas
    /// confirmée.
    #[serde(default)]
    pub host_key: Option<String>,
}

fn default_ssh_port() -> u16 {
    22
}

/// Réglages d'une destination, selon sa sorte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settings {
    S3(S3Settings),
    Webdav(WebDavSettings),
    Sftp(SftpSettings),
}

impl Settings {
    /// Lit et normalise les réglages d'une sorte. Un champ inconnu est refusé :
    /// un secret glissé là serait enregistré en clair.
    pub fn parse(kind: Kind, value: &Value) -> Result<Self, String> {
        let invalid = |error: untrusted::Rejected| format!("Invalid settings: {error}.");
        let settings = match kind {
            Kind::S3 => Self::S3(untrusted::from_value(value.clone()).map_err(invalid)?),
            Kind::Webdav => Self::Webdav(untrusted::from_value(value.clone()).map_err(invalid)?),
            Kind::Sftp => Self::Sftp(untrusted::from_value(value.clone()).map_err(invalid)?),
        };
        Ok(settings.normalized())
    }

    pub fn kind(&self) -> Kind {
        match self {
            Self::S3(_) => Kind::S3,
            Self::Webdav(_) => Kind::Webdav,
            Self::Sftp(_) => Kind::Sftp,
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Self::S3(settings) => serde_json::to_value(settings),
            Self::Webdav(settings) => serde_json::to_value(settings),
            Self::Sftp(settings) => serde_json::to_value(settings),
        }
        .unwrap_or_default()
    }

    fn normalized(self) -> Self {
        match self {
            Self::S3(mut s) => {
                s.endpoint = s.endpoint.trim().trim_end_matches('/').to_string();
                s.region = s.region.trim().to_ascii_lowercase();
                s.bucket = s.bucket.trim().to_string();
                s.prefix = s.prefix.trim().trim_matches('/').to_string();
                s.access_key_id = s.access_key_id.trim().to_string();
                Self::S3(s)
            }
            Self::Webdav(mut s) => {
                s.url = s.url.trim().to_string();
                s.username = s.username.trim().to_string();
                Self::Webdav(s)
            }
            Self::Sftp(mut s) => {
                s.host = s.host.trim().to_string();
                s.username = s.username.trim().to_string();
                let directory = s.directory.trim();
                s.directory = if directory == "/" {
                    "/".to_string()
                } else {
                    directory.trim_end_matches('/').to_string()
                };
                s.host_key =
                    s.host_key.map(|key| key.trim().to_string()).filter(|key| !key.is_empty());
                Self::Sftp(s)
            }
        }
    }

    /// Contrôle des réglages, message prêt pour l'interface.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::S3(s) => validate_s3(s),
            Self::Webdav(s) => {
                check_url(&s.url, "The WebDAV address")?;
                check_text(&s.username, "The user name", 256)
            }
            Self::Sftp(s) => {
                if s.host.is_empty()
                    || s.host.len() > 253
                    || s.host.chars().any(|c| c.is_whitespace() || c.is_control() || c == '/')
                {
                    return Err("Enter the host name or address of the SFTP server.".into());
                }
                if s.port == 0 {
                    return Err("The SFTP port must be between 1 and 65535.".into());
                }
                if s.username.is_empty() {
                    return Err("Enter the SFTP user name.".into());
                }
                check_text(&s.username, "The user name", 256)?;
                check_remote_path(&s.directory, "The remote folder")?;
                match &s.host_key {
                    Some(key) if !is_fingerprint(key) => Err(FINGERPRINT_HELP.into()),
                    _ => Ok(()),
                }
            }
        }
    }

    /// Ce qui désigne le serveur et le compte. Quand cela change, les secrets
    /// enregistrés ne suivent pas : ils partiraient vers un serveur qu'ils
    /// n'ont jamais connu.
    pub fn address(&self) -> String {
        match self {
            Self::S3(s) => {
                format!("s3|{}|{}|{}|{}", s.endpoint, s.region, s.bucket, s.access_key_id)
            }
            Self::Webdav(s) => format!("webdav|{}|{}", s.url, s.username),
            Self::Sftp(s) => format!("sftp|{}|{}|{}", s.host, s.port, s.username),
        }
    }

    /// Les secrets nécessaires à cette sorte sont là, et rien d'autre.
    pub fn check_secrets(&self, secrets: &Secrets) -> Result<(), String> {
        let allowed = self.kind().secret_fields();
        for (key, value) in secrets {
            if !allowed.contains(&key.as_str()) {
                return Err(format!("`{key}` is not a secret of this kind of destination."));
            }
            if value.len() > MAX_SECRET_LEN {
                return Err(format!("`{key}` is too long."));
            }
            // Une clé privée tient sur plusieurs lignes ; le reste, sur une.
            if key != "private_key" && value.chars().any(char::is_control) {
                return Err(format!("`{key}` contains a line break or a control character."));
            }
        }
        let has = |key: &str| secrets.get(key).is_some_and(|value| !value.is_empty());
        match self {
            Self::S3(_) if !has("secret_access_key") => Err("Enter the secret access key.".into()),
            Self::Sftp(_) if !has("password") && !has("private_key") => {
                Err("Enter a password or a private key.".into())
            }
            Self::Sftp(_) => match secrets.get("private_key").filter(|key| !key.is_empty()) {
                Some(key) => {
                    sftp::check_private_key(key, secrets.get("key_passphrase").map(String::as_str))
                }
                None => Ok(()),
            },
            _ => Ok(()),
        }
    }
}

/// Retire les secrets vides et les blancs autour d'une clé privée.
pub fn clean_secrets(secrets: Secrets) -> Secrets {
    secrets
        .into_iter()
        .map(|(key, value)| {
            let value = if key == "private_key" { value.trim().to_string() } else { value };
            (key, value)
        })
        .filter(|(_, value)| !value.is_empty())
        .collect()
}

fn validate_s3(s: &S3Settings) -> Result<(), String> {
    let bucket_ok = (3..=63).contains(&s.bucket.len())
        && s.bucket.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if !bucket_ok {
        return Err("Enter the bucket name: 3 to 63 letters, digits, dots or dashes.".into());
    }
    if s.access_key_id.is_empty()
        || s.access_key_id.len() > 256
        || s.access_key_id.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("Enter the access key ID.".into());
    }
    if s.region.len() > 64
        || !s.region.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("The region is a word like us-east-1, eu-central-1 or auto.".into());
    }
    if !s.endpoint.is_empty() {
        let url = check_url(&s.endpoint, "The endpoint")?;
        if url.path() != "/" && !url.path().is_empty() {
            return Err("The endpoint is the server address only, without a path.".into());
        }
        let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
        if !s.path_style && host.parse::<std::net::IpAddr>().is_ok() {
            return Err("An endpoint given by IP address needs path-style requests.".into());
        }
    }
    check_remote_path(&s.prefix, "The prefix")
}

/// Une adresse `http(s)://hôte…`, sans identifiants (ils ont leurs champs),
/// sans requête ni fragment.
fn check_url(raw: &str, what: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw)
        .map_err(|_| format!("{what} must be a full address, like https://host/…"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(format!("{what} must start with https:// or http://."));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!(
            "{what} must not contain a user name or password: use the fields for them."
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(format!("{what} must not contain ? or #."));
    }
    Ok(url)
}

fn check_text(value: &str, what: &str, max: usize) -> Result<(), String> {
    if value.len() > max || value.chars().any(char::is_control) {
        return Err(format!("{what} is too long or contains a control character."));
    }
    Ok(())
}

/// Chemin distant : pas de caractère de contrôle, pas de `\`, pas de `..`.
pub fn check_remote_path(path: &str, what: &str) -> Result<(), String> {
    if path.len() > MAX_PATH_LEN
        || path.chars().any(|c| c.is_control() || c == '\\')
        || path.split('/').any(|segment| segment == "..")
    {
        return Err(format!("{what} must be a plain path, without .. or control characters."));
    }
    Ok(())
}

/// `SHA256:` suivi de base64, comme `ssh-keygen -lf` l'écrit.
pub fn is_fingerprint(value: &str) -> bool {
    value.strip_prefix("SHA256:").is_some_and(|rest| {
        (20..=100).contains(&rest.len())
            && rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
    })
}

// --------------------------------------------------------------------------
// Noms distants
// --------------------------------------------------------------------------

const KEY_SUFFIX: &str = ".key";
const DB_SUFFIX: &str = ".db";
const TEST_PREFIX: &str = "dumbmonit-connection-test-";

/// Un nom que nous envoyons : lettres, chiffres, `.`, `-`, `_`, sans point
/// au début. Garde de chaque transport, avant tout envoi ou suppression.
pub fn is_safe_remote_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

pub(crate) fn check_name(name: &str) -> Result<(), Failure> {
    if is_safe_remote_name(name) {
        Ok(())
    } else {
        Err(Failure::fatal(format!("refusing the remote name {name:?}")))
    }
}

/// `dumbmonit-AAAAMMJJ-HHMMSS.db` → `dumbmonit-AAAAMMJJ-HHMMSS.key`.
pub fn key_name(db_name: &str) -> Option<String> {
    if !local::is_backup_name(db_name) {
        return None;
    }
    db_name.strip_suffix(DB_SUFFIX).map(|stem| format!("{stem}{KEY_SUFFIX}"))
}

/// La clé d'une de nos sauvegardes : `dumbmonit-AAAAMMJJ-HHMMSS.key`.
pub fn is_key_name(name: &str) -> bool {
    name.strip_suffix(KEY_SUFFIX)
        .is_some_and(|stem| local::is_backup_name(&format!("{stem}{DB_SUFFIX}")))
}

/// Nom du fichier d'essai, unique à chaque essai.
pub fn test_file_name() -> String {
    format!("{TEST_PREFIX}{}.txt", hex::encode(rand::random::<[u8; 6]>()))
}

/// Contenu du fichier d'essai.
pub const TEST_CONTENT: &[u8] = b"DumbMonit destination test. Safe to delete.\n";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FINGERPRINT: &str = "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s";

    #[test]
    fn our_names_are_the_only_ones_we_send() {
        assert!(is_safe_remote_name("dumbmonit-20260922-190000.db"));
        assert!(is_safe_remote_name(&test_file_name()));
        for bad in ["", ".hidden", "a/b", "../x", "a b", "a\nb", "é.db", "x\\y"] {
            assert!(!is_safe_remote_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_key_goes_with_its_database() {
        assert_eq!(
            key_name("dumbmonit-20260922-190000.db").as_deref(),
            Some("dumbmonit-20260922-190000.key")
        );
        assert_eq!(key_name("other.db"), None);
        assert!(is_key_name("dumbmonit-20260922-190000.key"));
        assert!(!is_key_name("dumbmonit-20260922-190000.db"));
        assert!(!is_key_name("secret.key"));
        assert!(!is_key_name("dumbmonit-2026-190000.key"));
    }

    #[test]
    fn the_test_file_never_looks_like_a_backup() {
        let name = test_file_name();
        assert!(!local::is_backup_name(&name) && !is_key_name(&name));
        assert_ne!(name, test_file_name(), "one name per test");
    }

    #[test]
    fn settings_are_normalized_and_checked() {
        let raw = json!({
            "endpoint": " https://minio.lan:9000/ ",
            "bucket": "backups",
            "prefix": "/dumbmonit/home/",
            "access_key_id": " AK ",
            "path_style": true
        });
        let Settings::S3(s3) = Settings::parse(Kind::S3, &raw).unwrap() else { panic!() };
        assert_eq!(s3.endpoint, "https://minio.lan:9000");
        assert_eq!(s3.prefix, "dumbmonit/home");
        assert_eq!(s3.access_key_id, "AK");
        assert!(Settings::S3(s3).validate().is_ok());

        // Un secret glissé dans les réglages est refusé, pas enregistré en clair.
        let leaked = json!({ "bucket": "b", "access_key_id": "a", "secret_access_key": "s" });
        assert!(Settings::parse(Kind::S3, &leaked).is_err());
    }

    #[test]
    fn unsafe_settings_are_refused() {
        let s3 = |patch: Value| {
            let mut base = json!({ "bucket": "backups", "access_key_id": "AK" });
            base.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
            Settings::parse(Kind::S3, &base).unwrap().validate()
        };
        assert!(s3(json!({})).is_ok());
        assert!(s3(json!({ "prefix": "a/../b" })).is_err());
        assert!(s3(json!({ "bucket": "x" })).is_err());
        assert!(s3(json!({ "endpoint": "ftp://host" })).is_err());
        assert!(s3(json!({ "endpoint": "https://user:pw@host" })).is_err());
        assert!(s3(json!({ "endpoint": "https://host/sub" })).is_err());
        assert!(s3(json!({ "endpoint": "http://10.0.0.5:9000" })).is_err(), "needs path style");
        assert!(s3(json!({ "endpoint": "http://10.0.0.5:9000", "path_style": true })).is_ok());
        assert!(s3(json!({ "region": "EU WEST" })).is_err());

        let webdav = json!({ "url": "https://cloud.example/remote.php/dav/files/me/b?x=1" });
        assert!(Settings::parse(Kind::Webdav, &webdav).unwrap().validate().is_err());

        let sftp = |patch: Value| {
            let mut base = json!({ "host": "nas.lan", "username": "backup" });
            base.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
            Settings::parse(Kind::Sftp, &base).unwrap().validate()
        };
        assert!(sftp(json!({ "directory": "/volume1/backups/" })).is_ok());
        assert!(sftp(json!({ "directory": "../etc" })).is_err());
        assert!(sftp(json!({ "host": "nas lan" })).is_err());
        assert!(sftp(json!({ "port": 0 })).is_err());
        assert!(sftp(json!({ "host_key": "md5:aa" })).is_err());
        assert!(sftp(json!({ "host_key": FINGERPRINT })).is_ok());
    }

    #[test]
    fn secrets_must_match_the_kind() {
        let s3 = Settings::S3(S3Settings {
            bucket: "backups".into(),
            access_key_id: "AK".into(),
            ..S3Settings::default()
        });
        let secrets = |pairs: &[(&str, &str)]| -> Secrets {
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
        };
        assert!(s3.check_secrets(&secrets(&[("secret_access_key", "s")])).is_ok());
        assert!(s3.check_secrets(&secrets(&[])).is_err(), "required");
        assert!(s3.check_secrets(&secrets(&[("password", "p")])).is_err(), "foreign field");

        let sftp = Settings::Sftp(SftpSettings {
            host: "h".into(),
            port: 22,
            username: "u".into(),
            ..SftpSettings::default()
        });
        assert!(sftp.check_secrets(&secrets(&[("password", "p")])).is_ok());
        assert!(sftp.check_secrets(&secrets(&[])).is_err(), "password or key");
        assert!(sftp.check_secrets(&secrets(&[("password", "a\nb")])).is_err());
        assert!(sftp.check_secrets(&secrets(&[("private_key", "not a key")])).is_err());

        let cleaned = clean_secrets(secrets(&[("password", ""), ("private_key", "  k \n")]));
        assert_eq!(cleaned, secrets(&[("private_key", "k")]));
    }

    #[test]
    fn the_address_ignores_what_does_not_designate_the_server() {
        let a = Settings::Sftp(SftpSettings {
            host: "h".into(),
            port: 22,
            username: "u".into(),
            directory: "a".into(),
            host_key: None,
        });
        let Settings::Sftp(mut moved) = a.clone() else { unreachable!() };
        moved.directory = "b".into();
        assert_eq!(a.address(), Settings::Sftp(moved.clone()).address());
        moved.host = "other".into();
        assert_ne!(a.address(), Settings::Sftp(moved).address());
    }
}
