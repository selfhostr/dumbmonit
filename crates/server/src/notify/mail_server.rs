//! Serveur de courriel de l'instance : un SMTP réglé une fois dans les
//! réglages, pour tout ce qui part par courriel sans canal choisi — abonnés
//! des pages de statut, rapports planifiés.
//!
//! Le réglage vit dans `settings` : les champs en clair sous [`SETTINGS_KEY`],
//! le mot de passe à part, chiffré par la clé d'instance, sous [`PASSWORD_KEY`].
//! Il ne fait jamais l'aller-retour : l'API dit seulement `has_password`.
//!
//! Les variables `DUMBMONIT_SMTP_*` décrivent le même serveur. Dès que
//! `DUMBMONIT_SMTP_HOST` est renseignée, elles l'emportent sur le réglage
//! enregistré, et l'interface montre les champs verrouillés : l'opérateur qui
//! déploie par fichier compose garde la main.
//!
//! L'envoi reprend le transport des canaux courriel ([`super::smtp::Smtp`]) :
//! même chiffrement, même garde contre les adresses internes, même expurgation
//! du mot de passe dans les erreurs.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;

use crate::crypto::Cipher;
use crate::notify::channel::ChannelConfig;

/// Champs en clair du réglage, dans `settings`.
pub const SETTINGS_KEY: &str = "mail.server";
/// Mot de passe SMTP, chiffré, dans `settings`.
pub const PASSWORD_KEY: &str = "mail.server.password";

const MAX_HOST_LEN: usize = 253;
const MAX_TEXT_LEN: usize = 254;
const MAX_NAME_LEN: usize = 120;
const MAX_PASSWORD_LEN: usize = 1024;

/// Chiffrement de la connexion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MailSecurity {
    /// Connexion en clair puis `STARTTLS`, port 587.
    #[default]
    #[serde(rename = "starttls")]
    StartTls,
    /// TLS dès la connexion, port 465.
    #[serde(rename = "tls")]
    Tls,
    /// Aucun chiffrement, port 25 : seulement vers un relais local.
    #[serde(rename = "none")]
    Plain,
}

impl MailSecurity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StartTls => "starttls",
            Self::Tls => "tls",
            Self::Plain => "none",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::StartTls => 587,
            Self::Tls => 465,
            Self::Plain => 25,
        }
    }

    /// Valeur d'une variable d'environnement ; `None` si elle est inconnue.
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "starttls" | "" => Some(Self::StartTls),
            "tls" | "ssl" | "implicit" => Some(Self::Tls),
            "none" | "plain" | "off" => Some(Self::Plain),
            _ => None,
        }
    }
}

/// Le serveur, sans son mot de passe.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailServer {
    pub host: String,
    /// `None` : le port habituel du chiffrement choisi.
    pub port: Option<u16>,
    pub security: MailSecurity,
    /// Vide : pas d'authentification.
    pub username: String,
    pub from_address: String,
    /// Nom d'affichage de l'expéditeur ; vide : l'adresse seule.
    pub from_name: String,
}

impl MailServer {
    /// Assez renseigné pour envoyer : un hôte et une adresse d'expéditeur.
    pub fn is_configured(&self) -> bool {
        !self.host.trim().is_empty() && !self.from_address.trim().is_empty()
    }

    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or_else(|| self.security.default_port())
    }

    /// Les champs, nettoyés des espaces autour.
    pub fn trimmed(self) -> Self {
        Self {
            host: self.host.trim().to_string(),
            port: self.port,
            security: self.security,
            username: self.username.trim().to_string(),
            from_address: self.from_address.trim().to_string(),
            from_name: self.from_name.trim().to_string(),
        }
    }

    /// Contrôle d'une saisie. Tout vide est admis : c'est « pas de serveur ».
    pub fn validate(&self) -> Result<(), String> {
        if self.host.is_empty() && self.from_address.is_empty() {
            return Ok(());
        }
        if self.host.is_empty() {
            return Err("Enter the host name of the email server.".into());
        }
        if self.host.len() > MAX_HOST_LEN
            || self.host.contains("://")
            || self.host.chars().any(|c| c.is_whitespace() || c.is_control() || c == '/')
        {
            return Err("The host is a name or an address, without scheme or path.".into());
        }
        if self.port == Some(0) {
            return Err("The port must be between 1 and 65535.".into());
        }
        if self.from_address.is_empty() {
            return Err("Enter the address the emails are sent from.".into());
        }
        if self.from_address.len() > MAX_TEXT_LEN
            || self.from_address.parse::<lettre::Address>().is_err()
        {
            return Err("The sender address is not a valid email address.".into());
        }
        if self.username.len() > MAX_TEXT_LEN || self.username.chars().any(char::is_control) {
            return Err("The username is too long or holds control characters.".into());
        }
        let name = &self.from_name;
        if name.chars().count() > MAX_NAME_LEN || name.chars().any(char::is_control) {
            return Err(format!(
                "The sender name is limited to {MAX_NAME_LEN} characters of plain text."
            ));
        }
        Ok(())
    }

    /// Le serveur sous la forme d'un canal courriel, sans destinataire : c'est
    /// ce que prend [`super::smtp::Smtp::sender`].
    pub fn channel(&self, password: Option<&str>) -> ChannelConfig {
        let mut settings = json!({
            "host": self.host,
            "port": self.effective_port(),
            "security": self.security.as_str(),
            "from": self.from_address,
        });
        if !self.username.is_empty() {
            settings["username"] = json!(self.username);
        }
        if !self.from_name.is_empty() {
            settings["from_name"] = json!(self.from_name);
        }
        let secrets = match password.filter(|p| !p.is_empty()) {
            Some(password) => json!({ "password": password }),
            None => json!({}),
        };
        ChannelConfig {
            id: 0,
            name: "Email server".to_string(),
            kind: "smtp".to_string(),
            enabled: true,
            settings,
            secrets,
        }
    }
}

pub fn check_password(password: &str) -> Result<(), String> {
    if password.len() > MAX_PASSWORD_LEN || password.chars().any(char::is_control) {
        return Err("The password is too long or holds control characters.".into());
    }
    Ok(())
}

/// Le serveur décrit par l'environnement, et son mot de passe. `None` sans
/// `DUMBMONIT_SMTP_HOST`.
pub fn from_env() -> Option<(MailServer, Option<String>)> {
    from_env_with(crate::config::env_var)
}

/// [`from_env`] sur une source arbitraire (les tests n'ont pas à toucher à
/// l'environnement du processus). Un port ou un chiffrement illisible est
/// signalé et remplacé par le défaut, plutôt que de couper les envois.
pub fn from_env_with(
    read: impl Fn(&str) -> Option<String>,
) -> Option<(MailServer, Option<String>)> {
    let read = |key: &str| {
        read(key).map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
    };
    let host = read("DUMBMONIT_SMTP_HOST")?;
    let security = match read("DUMBMONIT_SMTP_SECURITY") {
        None => MailSecurity::default(),
        Some(raw) => MailSecurity::parse(&raw).unwrap_or_else(|| {
            tracing::warn!(value = %raw, "DUMBMONIT_SMTP_SECURITY unknown, STARTTLS used");
            MailSecurity::default()
        }),
    };
    let port = read("DUMBMONIT_SMTP_PORT").and_then(|raw| match raw.parse::<u16>() {
        Ok(port) if port > 0 => Some(port),
        _ => {
            tracing::warn!(value = %raw, "DUMBMONIT_SMTP_PORT unreadable, default port used");
            None
        }
    });
    let server = MailServer {
        host,
        port,
        security,
        username: read("DUMBMONIT_SMTP_USERNAME").unwrap_or_default(),
        from_address: read("DUMBMONIT_SMTP_FROM").unwrap_or_default(),
        from_name: read("DUMBMONIT_SMTP_FROM_NAME").unwrap_or_default(),
    };
    Some((server, read("DUMBMONIT_SMTP_PASSWORD")))
}

/// Réglage enregistré, mot de passe à part. `None` : rien d'enregistré.
pub async fn load_stored(pool: &SqlitePool) -> Result<Option<MailServer>> {
    crate::auth::settings::get::<MailServer>(pool, SETTINGS_KEY).await
}

pub async fn load_password(pool: &SqlitePool, cipher: &Cipher) -> Result<Option<String>> {
    crate::auth::settings::get_secret(pool, cipher, PASSWORD_KEY).await
}

/// Ce que devient le mot de passe enregistré.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordChange {
    Keep,
    Clear,
    Set(String),
}

/// Enregistre le serveur. Un serveur vide efface tout, mot de passe compris.
pub async fn save(
    pool: &SqlitePool,
    cipher: &Cipher,
    server: &MailServer,
    password: PasswordChange,
) -> Result<()> {
    if server.host.is_empty() && server.from_address.is_empty() {
        crate::auth::settings::delete(pool, SETTINGS_KEY).await?;
        crate::auth::settings::delete(pool, PASSWORD_KEY).await?;
        return Ok(());
    }
    crate::auth::settings::set(pool, SETTINGS_KEY, server).await?;
    match password {
        PasswordChange::Keep => {}
        PasswordChange::Clear => crate::auth::settings::delete(pool, PASSWORD_KEY).await?,
        PasswordChange::Set(password) => {
            crate::auth::settings::set_secret(pool, cipher, PASSWORD_KEY, &password).await?;
        }
    }
    Ok(())
}

/// Le serveur en vigueur et d'où il vient. Pas de `Debug` : il porte le mot
/// de passe en clair.
#[derive(Clone)]
pub struct Resolved {
    pub server: MailServer,
    pub password: Option<String>,
    /// Décrit par `DUMBMONIT_SMTP_*` : le réglage enregistré est ignoré.
    pub from_env: bool,
}

impl Resolved {
    pub fn is_configured(&self) -> bool {
        self.server.is_configured()
    }

    pub fn channel(&self) -> ChannelConfig {
        self.server.channel(self.password.as_deref())
    }
}

pub async fn resolve(pool: &SqlitePool, cipher: &Cipher) -> Result<Resolved> {
    if let Some((server, password)) = from_env() {
        return Ok(Resolved { server, password, from_env: true });
    }
    let server = load_stored(pool).await?.unwrap_or_default();
    let password = if server.is_configured() {
        load_password(pool, cipher).await.context("mot de passe du serveur de courriel")?
    } else {
        None
    };
    Ok(Resolved { server, password, from_env: false })
}

/// Le serveur de courriel prêt à l'envoi, sous la forme d'un canal ; `None`
/// s'il n'est pas réglé (ou illisible, signalé dans le journal).
pub async fn channel(pool: &SqlitePool, cipher: &Cipher) -> Option<ChannelConfig> {
    match resolve(pool, cipher).await {
        Ok(resolved) if resolved.is_configured() => Some(resolved.channel()),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(%error, "email server settings unreadable");
            None
        }
    }
}

#[cfg(test)]
mod tests;
