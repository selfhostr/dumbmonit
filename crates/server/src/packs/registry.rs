//! Registre communautaire de paquets : un `index.json` hébergé n'importe où
//! (GitHub Pages, un dépôt, un serveur interne) qui liste des paquets et
//! l'adresse de leur YAML.
//!
//! Le registre est une option : l'adresse est vide tant que l'administrateur
//! n'en a pas saisi une. Tout ce qui est lu passe par le garde des envois
//! sortants (jamais vers le serveur lui-même), avec une taille maximale. Un
//! paquet n'est installé que si son empreinte SHA-256 correspond à celle de
//! l'index et si sa licence est dans la liste permise.

use std::time::Duration;

use anyhow::Result;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::auth::settings;
use crate::notify::guard;

/// Clé du réglage dans la table `settings`.
pub const SETTINGS_KEY: &str = "packs.registry";

/// Taille maximale de l'index.
pub const MAX_INDEX_BYTES: usize = 1024 * 1024;

/// Taille maximale d'un paquet téléchargé.
pub const MAX_PACK_BYTES: usize = 512 * 1024;

/// Délai maximal d'un téléchargement.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Réglage du registre : une adresse, vide tant que rien n'est configuré.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub url: String,
}

pub async fn load(pool: &SqlitePool) -> Result<Settings> {
    Ok(settings::get::<Settings>(pool, SETTINGS_KEY).await?.unwrap_or_default())
}

pub async fn save(pool: &SqlitePool, value: &Settings) -> Result<()> {
    settings::set(pool, SETTINGS_KEY, value).await
}

/// Contrôle l'adresse saisie : vide (registre coupé) ou une URL http(s) sans
/// identifiants. Renvoie l'adresse nettoyée.
pub fn validate_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    if raw.len() > 2048 {
        return Err("The registry address is too long.".into());
    }
    let url =
        Url::parse(raw).map_err(|_| "The registry address is not a valid URL.".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("The registry address must start with http:// or https://.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("The registry address must not contain a user name or password.".into());
    }
    Ok(url.to_string())
}

/// Une ligne de l'index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    pub license: String,
    /// Empreinte SHA-256 (hexadécimal) du YAML.
    pub sha256: String,
    /// Adresse du YAML, absolue ou relative à l'index.
    pub url: String,
    /// Plus ancienne version de DumbMonit qui sait lire ce paquet.
    #[serde(default)]
    pub min_version: Option<String>,
}

#[derive(Deserialize)]
struct RawIndex {
    packs: Vec<serde_json::Value>,
}

/// Lit un index. Une ligne illisible est écartée et signalée, elle ne fait pas
/// tomber les autres ; un index qui n'est pas du tout un index est refusé.
pub fn parse_index(bytes: &[u8]) -> Result<(Vec<Entry>, Vec<String>), String> {
    let raw: RawIndex = serde_json::from_slice(bytes).map_err(
        |_| "The registry index is not a valid index.json (expected {\"packs\": [...]}).",
    )?;
    let mut entries: Vec<Entry> = Vec::new();
    let mut skipped = Vec::new();
    for value in raw.packs {
        let label = value.get("id").and_then(|id| id.as_str()).unwrap_or("?").to_string();
        match serde_json::from_value::<Entry>(value) {
            Ok(entry) => match check_entry(&entry) {
                Ok(()) if entries.iter().any(|known| known.id == entry.id) => {
                    skipped.push(format!("{label}: listed twice"));
                }
                Ok(()) => entries.push(entry),
                Err(reason) => skipped.push(format!("{label}: {reason}")),
            },
            Err(_) => skipped.push(format!("{label}: missing or invalid fields")),
        }
    }
    Ok((entries, skipped))
}

fn check_entry(entry: &Entry) -> Result<(), String> {
    if entry.id.trim().is_empty() || entry.id.len() > 64 {
        return Err("invalid id".into());
    }
    if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("sha256 must be 64 hexadecimal characters".into());
    }
    if entry.url.trim().is_empty() {
        return Err("no url".into());
    }
    Ok(())
}

/// Licences permises : permissives ou à copyleft faible, l'identifiant SPDX
/// seul (pas d'expression `OR`/`AND`).
pub fn license_allowed(license: &str) -> bool {
    let license = license.trim().to_ascii_lowercase();
    if let Some(rest) = license.strip_prefix("bsd-") {
        return !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    }
    matches!(
        license.as_str(),
        "mit" | "apache-2.0" | "isc" | "mpl-2.0" | "cc0-1.0" | "cc0" | "unlicense"
    )
}

/// Vrai si la version de l'index est strictement plus récente que celle installée.
pub fn is_newer(available: &str, installed: &str) -> bool {
    use dumbmonit_proto::Version;
    match (Version::parse(available), Version::parse(installed)) {
        (Some(available), Some(installed)) => available > installed,
        _ => false,
    }
}

/// Vrai si cette version de DumbMonit satisfait la version minimale du paquet.
/// Une version minimale illisible ne bloque rien.
pub fn compatible(min_version: Option<&str>, current: &str) -> bool {
    use dumbmonit_proto::Version;
    match (min_version.and_then(Version::parse), Version::parse(current)) {
        (Some(min), Some(current)) => current >= min,
        _ => true,
    }
}

/// Compare l'empreinte annoncée à celle des octets reçus.
pub fn verify_sha256(bytes: &[u8], expected: &str) -> Result<(), String> {
    let actual = hex::encode(Sha256::digest(bytes));
    if actual.eq_ignore_ascii_case(expected.trim()) {
        Ok(())
    } else {
        Err("The downloaded pack does not match the checksum announced by the registry.".into())
    }
}

/// Résout l'adresse d'un paquet par rapport à celle de l'index.
pub fn resolve(index_url: &str, entry_url: &str) -> Result<Url, String> {
    let base =
        Url::parse(index_url).map_err(|_| "The registry address is not valid.".to_string())?;
    let url =
        base.join(entry_url.trim()).map_err(|_| "The pack address is not valid.".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("The pack address must use http or https.".into());
    }
    Ok(url)
}

/// Télécharge une adresse, bornée en taille et en durée, sans jamais joindre le
/// serveur lui-même (garde des envois sortants, redirections comprises).
pub async fn fetch(url: &Url, max_bytes: usize) -> Result<Vec<u8>, String> {
    guard::vet_url(url).await.map_err(|error| error.to_string())?;
    let client = crate::notify::http_client();
    let port = url.port_or_known_default();
    let request = client.get(url.clone()).timeout(FETCH_TIMEOUT).send();
    let mut response =
        guard::scoped(port, request).await.map_err(|error| match guard::refusal_in(&error) {
            Some(refused) => refused.to_string(),
            None => "The registry could not be reached.".to_string(),
        })?;
    if !response.status().is_success() {
        return Err(format!("The registry answered {}.", response.status()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = guard::scoped(port, response.chunk())
        .await
        .map_err(|_| "The registry's answer was cut short.".to_string())?
    {
        if body.len() + chunk.len() > max_bytes {
            return Err("The registry's answer is larger than allowed.".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, sha: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "name": "N", "version": "1.0.0", "license": "MIT",
            "sha256": sha, "url": "packs/x.yaml"
        })
    }

    #[test]
    fn an_index_is_read_and_bad_lines_are_skipped() {
        let good = "a".repeat(64);
        let doc = serde_json::json!({ "packs": [
            entry("one", &good), entry("one", &good), entry("bad", "xyz"),
            { "id": "broken" }
        ]});
        let (entries, skipped) = parse_index(doc.to_string().as_bytes()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "one");
        assert_eq!(skipped.len(), 3);
        assert!(parse_index(b"[]").is_err());
        assert!(parse_index(b"not json").is_err());
    }

    #[test]
    fn versions_are_compared_as_semver() {
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
        assert!(is_newer("1.0.0", "1.0.0-beta.1"));
        assert!(!is_newer("garbage", "1.0.0"));
        assert!(compatible(Some("0.20.0"), "0.21.1"));
        assert!(!compatible(Some("0.30.0"), "0.21.1"));
        assert!(compatible(None, "0.21.1"));
    }

    #[test]
    fn only_permissive_licences_are_allowed() {
        let allowed =
            ["MIT", "apache-2.0", "BSD-3-Clause", "BSD-2-Clause", "ISC", "MPL-2.0", "CC0-1.0"];
        for ok in allowed.into_iter().chain(["Unlicense"]) {
            assert!(license_allowed(ok), "{ok}");
        }
        let refused = ["GPL-3.0", "AGPL-3.0-only", "proprietary", "", "BSD-", "MIT OR GPL-3.0"];
        for no in refused {
            assert!(!license_allowed(no), "{no}");
        }
    }

    #[test]
    fn a_checksum_mismatch_is_refused() {
        let sum = hex::encode(Sha256::digest(b"hello"));
        assert!(verify_sha256(b"hello", &sum).is_ok());
        assert!(verify_sha256(b"hello", &sum.to_uppercase()).is_ok());
        assert!(verify_sha256(b"hellO", &sum).is_err());
    }

    #[test]
    fn addresses_are_validated_and_resolved() {
        assert_eq!(validate_url("  ").unwrap(), "");
        assert!(validate_url("ftp://x/index.json").is_err());
        assert!(validate_url("https://user:pw@x/index.json").is_err());
        assert!(validate_url("nope").is_err());
        assert!(validate_url("https://x.github.io/reg/index.json").is_ok());
        let url = resolve("https://x.io/reg/index.json", "packs/a.yaml").unwrap();
        assert_eq!(url.as_str(), "https://x.io/reg/packs/a.yaml");
        assert!(resolve("https://x.io/reg/index.json", "file:///etc/passwd").is_err());
    }
}
