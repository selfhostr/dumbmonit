//! Politique d'authentification de l'instance.
//!
//! Un seul réglage pour l'instant : exiger le second facteur (TOTP) des
//! administrateurs et des opérateurs. Activé, un tel compte qui se connecte par
//! mot de passe sans second facteur ne peut plus que l'enrôler (ou se
//! déconnecter) : toute autre route d'API lui répond `403` avec le code
//! [`TOTP_SETUP_REQUIRED`], et l'interface l'emmène à l'enrôlement.
//!
//! Exemptées : les sessions ouvertes par le SSO (OIDC, SAML) ou l'annuaire
//! (LDAP) — c'est le fournisseur d'identité qui porte la MFA — et par une clé
//! d'accès, qui vaut à elle seule possession et vérification de l'utilisateur.
//! Un compte sans mot de passe local ne peut d'ailleurs pas enrôler de TOTP.
//!
//! Jamais visés : les lecteurs, et les écrans muraux (rôle `wall`). Un écran
//! fixé au mur n'a personne pour taper un code, et son compte ne joint que
//! ce que lit le mur (`wall::WALL_ROUTES`) ; l'enrôlement lui est d'ailleurs
//! fermé.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::auth::session::Via;
use crate::auth::settings;
use crate::auth::users::User;

/// Clé du réglage dans `settings` (JSON en clair).
pub const SETTINGS_KEY: &str = "auth.policy";

/// Code d'erreur des refus dus à la politique, lu par l'interface.
pub const TOTP_SETUP_REQUIRED: &str = "totp_setup_required";

/// Le réglage tel qu'il est rangé et exposé.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Exiger le second facteur des administrateurs et des opérateurs.
    #[serde(default)]
    pub require_totp: bool,
}

pub async fn load(pool: &SqlitePool) -> Result<Policy> {
    Ok(settings::get::<Policy>(pool, SETTINGS_KEY)
        .await
        .context("lecture de la politique d'authentification")?
        .unwrap_or_default())
}

pub async fn save(pool: &SqlitePool, policy: &Policy) -> Result<()> {
    settings::set(pool, SETTINGS_KEY, policy).await
}

/// Vrai si la politique *pourrait* viser ce compte dans cette session : rôle
/// admin ou opérateur (ou rôle personnalisé qui écrit), mot de passe local,
/// pas de second facteur, session ouverte par mot de passe (ou d'avant que la
/// façon soit retenue). Pur, pour
/// que le garde ne lise le réglage que dans ce cas-là, qui est rare.
pub fn may_apply(user: &User, via: Option<Via>) -> bool {
    (user.role.can_operate() || crate::auth::permissions::writes_anything(user))
        && !user.totp_enabled
        && user.password_hash.is_some()
        && via.is_none_or(|via| !via.exempt_from_totp_policy())
}

/// Le compte doit enrôler son second facteur avant toute autre chose.
pub async fn setup_required(pool: &SqlitePool, user: &User, via: Option<Via>) -> Result<bool> {
    if !may_apply(user, via) {
        return Ok(false);
    }
    Ok(load(pool).await?.require_totp)
}

/// Routes qu'un compte bloqué par la politique peut encore appeler : se voir,
/// enrôler son second facteur, se déconnecter. Chemins sans le préfixe `/api`.
pub fn allowed_while_blocked(method: &axum::http::Method, path: &str) -> bool {
    use axum::http::Method;
    let path = path.strip_prefix("/api").unwrap_or(path);
    match path {
        "/auth/me" => *method == Method::GET,
        "/auth/totp" => *method == Method::GET,
        "/auth/totp/enroll" | "/auth/totp/verify" | "/auth/logout" => *method == Method::POST,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
