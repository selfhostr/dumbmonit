//! Réinitialisation du mot de passe par lien, et réglages des liens de compte.
//!
//! Deux chemins mènent au même lien à usage unique (`/reset?token=…`) : « Mot
//! de passe oublié ? » sur l'écran de connexion, qui l'envoie par courriel, et
//! l'administrateur, qui le crée depuis la liste des comptes (envoyé par
//! courriel, ou copié et transmis à la main).
//!
//! Seul un compte à mot de passe local y a droit : un compte SSO ou LDAP
//! change son mot de passe chez son fournisseur, et un écran mural (`wall`)
//! n'a personne pour lire un courriel — son mot de passe se remet depuis la
//! liste des comptes. Utiliser le lien remplace le mot de passe, ferme toutes
//! les sessions du compte et invalide les autres liens en cours. Le second
//! facteur, lui, reste : le lien prouve l'accès à la boîte, pas au téléphone.
//!
//! Les liens partent vers l'URL publique de l'instance, sinon vers l'origine
//! qu'un administrateur connecté a utilisée en dernier ([`LINK_ORIGIN_KEY`]).
//! Jamais vers l'en-tête `Host` d'une requête anonyme : n'importe qui pourrait
//! faire envoyer à la victime un lien qui mène chez lui.

use std::ops::RangeInclusive;

use anyhow::{Context, Result};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::auth::link_token::LinkToken;
use crate::auth::settings;
use crate::auth::users::User;

/// Réglages des liens de compte, dans `settings` (JSON en clair).
pub const SETTINGS_KEY: &str = "auth.recovery";
/// Dernière origine HTTPS vue d'un administrateur connecté, base des liens
/// quand aucune URL publique n'est réglée.
pub const LINK_ORIGIN_KEY: &str = "auth.link_origin";

/// Bornes de la durée de vie d'un lien de réinitialisation, en minutes.
pub const RESET_MINUTES: RangeInclusive<u32> = 5..=1440;
/// Bornes de la durée de vie d'une invitation, en heures (30 jours au plus).
pub const INVITE_HOURS: RangeInclusive<u32> = 1..=720;

/// Ce que l'administrateur règle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecoverySettings {
    /// « Mot de passe oublié ? » envoie un lien par courriel. Coupé, l'écran
    /// de connexion renvoie à la documentation, comme sans serveur de courriel.
    pub self_service: bool,
    /// Durée de vie d'un lien de réinitialisation.
    pub reset_minutes: u32,
    /// Durée de vie d'une invitation.
    pub invite_hours: u32,
}

impl Default for RecoverySettings {
    fn default() -> Self {
        Self { self_service: true, reset_minutes: 30, invite_hours: 7 * 24 }
    }
}

impl RecoverySettings {
    pub fn validate(&self) -> Result<(), String> {
        if !RESET_MINUTES.contains(&self.reset_minutes) {
            return Err(format!(
                "A reset link lasts between {} and {} minutes.",
                RESET_MINUTES.start(),
                RESET_MINUTES.end()
            ));
        }
        if !INVITE_HOURS.contains(&self.invite_hours) {
            return Err(format!(
                "An invitation lasts between {} and {} hours.",
                INVITE_HOURS.start(),
                INVITE_HOURS.end()
            ));
        }
        Ok(())
    }
}

/// Le réglage en vigueur ; illisible, les valeurs par défaut (signalé).
pub async fn load(pool: &SqlitePool) -> Result<RecoverySettings> {
    match settings::get::<RecoverySettings>(pool, SETTINGS_KEY).await {
        Ok(stored) => Ok(stored.unwrap_or_default()),
        Err(error) => {
            tracing::warn!(?error, "account link settings unreadable, defaults used");
            Ok(RecoverySettings::default())
        }
    }
}

pub async fn save(pool: &SqlitePool, value: &RecoverySettings) -> Result<()> {
    settings::set(pool, SETTINGS_KEY, value).await
}

/// Vrai si un lien de réinitialisation peut viser ce compte : actif, avec un
/// mot de passe local, et pas un écran mural.
pub fn can_reset(user: &User) -> bool {
    !user.disabled && user.password_hash.is_some() && !user.role.is_wall()
}

/// Vrai si « Mot de passe oublié ? » peut lui envoyer un lien : il faut en
/// plus une adresse où l'envoyer.
pub fn can_reset_by_email(user: &User) -> bool {
    can_reset(user) && user.email.is_some()
}

// ---------------------------------------------------------------------------
// Base des liens
// ---------------------------------------------------------------------------

/// Base des liens envoyés : l'URL publique (réglage ou `DUMBMONIT_PUBLIC_URL`,
/// en HTTPS), sinon la dernière origine d'un administrateur connecté.
pub async fn link_base(pool: &SqlitePool) -> Option<String> {
    let global = crate::notify::policy_store::load_global(pool).await.ok();
    let env_url = crate::config::env_var("DUMBMONIT_PUBLIC_URL");
    if let Some(url) = global.and_then(|global| global.public_url(env_url.as_deref())) {
        return Some(url);
    }
    let stored = settings::get::<String>(pool, LINK_ORIGIN_KEY).await.ok().flatten()?;
    clean_origin(&stored)
}

/// Retient l'origine d'un administrateur connecté (`https://hôte[:port]`).
/// Une valeur d'une autre forme est ignorée.
pub async fn remember_origin(pool: &SqlitePool, origin: &str) {
    let Some(origin) = clean_origin(origin) else { return };
    let known = settings::get::<String>(pool, LINK_ORIGIN_KEY).await.ok().flatten();
    if known.as_deref() == Some(origin.as_str()) {
        return;
    }
    if let Err(error) = settings::set(pool, LINK_ORIGIN_KEY, &origin).await {
        tracing::warn!(?error, "administrator origin not remembered");
    }
}

/// `https://hôte[:port]`, sans chemin ni barre finale ; `None` sinon.
fn clean_origin(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let host = trimmed.strip_prefix("https://")?;
    let valid = !host.is_empty()
        && host.len() <= 260
        && !host.contains(['/', '?', '#', '@', '\\'])
        && !host.chars().any(|c| c.is_whitespace() || c.is_control());
    valid.then(|| trimmed.to_string())
}

/// Lien de réinitialisation sous `base`.
pub fn reset_url(base: &str, token: &LinkToken) -> String {
    format!("{base}/reset?token={}", token.value())
}

// ---------------------------------------------------------------------------
// Liens de réinitialisation
// ---------------------------------------------------------------------------

/// Un lien tout juste créé : le jeton n'existe en clair qu'ici.
pub struct Issued {
    pub token: LinkToken,
    pub expires_at: DateTime<Utc>,
}

fn stamp(at: DateTime<Utc>) -> String {
    crate::db::timestamp(at)
}

/// Crée un lien pour le compte. Les liens encore valides du même compte sont
/// retirés : seul le dernier envoyé fonctionne.
pub async fn issue(
    pool: &SqlitePool,
    user_id: i64,
    requested_by: &str,
    minutes: u32,
    now: DateTime<Utc>,
) -> Result<Issued> {
    let token = LinkToken::generate();
    let expires_at = now + TimeDelta::minutes(i64::from(minutes));
    let mut tx = pool.begin().await.context("ouverture de la transaction")?;
    // Ménage : les liens utilisés ou expirés depuis une semaine ne servent plus
    // à rien, pas même au diagnostic.
    sqlx::query("DELETE FROM password_resets WHERE expires_at < ?")
        .bind(stamp(now - TimeDelta::days(7)))
        .execute(&mut *tx)
        .await
        .context("purge des liens de réinitialisation")?;
    sqlx::query("DELETE FROM password_resets WHERE user_id = ? AND used_at IS NULL")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("retrait des liens précédents")?;
    sqlx::query(
        "INSERT INTO password_resets
             (id, token_hash, user_id, requested_by, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(token.id())
    .bind(token.secret_hash())
    .bind(user_id)
    .bind(requested_by)
    .bind(stamp(now))
    .bind(stamp(expires_at))
    .execute(&mut *tx)
    .await
    .context("création d'un lien de réinitialisation")?;
    tx.commit().await.context("validation du lien")?;
    Ok(Issued { token, expires_at })
}

/// Un lien valide : son compte et sa fin.
#[derive(Debug, Clone)]
pub struct Pending {
    pub user_id: i64,
    pub expires_at: String,
}

/// Retrouve un lien valide — connu, secret juste, ni utilisé ni expiré.
/// Toutes les autres issues donnent `None`, sans distinction.
pub async fn find(
    pool: &SqlitePool,
    token: &LinkToken,
    now: DateTime<Utc>,
) -> Result<Option<Pending>> {
    let row = sqlx::query(
        "SELECT token_hash, user_id, expires_at, used_at FROM password_resets WHERE id = ?",
    )
    .bind(token.id())
    .fetch_optional(pool)
    .await
    .context("lecture d'un lien de réinitialisation")?;
    let Some(row) = row else { return Ok(None) };
    let stored: Vec<u8> = row.try_get("token_hash")?;
    let expires_at: String = row.try_get("expires_at")?;
    let used_at: Option<String> = row.try_get("used_at")?;
    if !token.matches(&stored) || used_at.is_some() || expires_at <= stamp(now) {
        return Ok(None);
    }
    Ok(Some(Pending { user_id: row.try_get("user_id")?, expires_at }))
}

/// Utilise le lien : nouveau mot de passe, toutes les sessions fermées, les
/// autres liens du compte retirés — d'un seul bloc. `None` quand le lien n'est
/// plus valide (déjà utilisé, y compris par une requête concurrente) ou que le
/// compte n'y a plus droit ; rien n'est alors écrit.
pub async fn complete(
    pool: &SqlitePool,
    token: &LinkToken,
    password_hash: &str,
    now: DateTime<Utc>,
) -> Result<Option<i64>> {
    let mut tx = pool.begin().await.context("ouverture de la transaction")?;
    let row = sqlx::query(
        "SELECT token_hash, user_id, expires_at, used_at FROM password_resets WHERE id = ?",
    )
    .bind(token.id())
    .fetch_optional(&mut *tx)
    .await
    .context("lecture d'un lien de réinitialisation")?;
    let Some(row) = row else { return Ok(None) };
    let stored: Vec<u8> = row.try_get("token_hash")?;
    if !token.matches(&stored) {
        return Ok(None);
    }
    let user_id: i64 = row.try_get("user_id")?;
    // La condition est dans la requête : deux envois simultanés du même lien
    // ne peuvent pas passer tous les deux.
    let used = sqlx::query(
        "UPDATE password_resets SET used_at = ?
         WHERE id = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(stamp(now))
    .bind(token.id())
    .bind(stamp(now))
    .execute(&mut *tx)
    .await
    .context("consommation du lien")?;
    if used.rows_affected() != 1 {
        return Ok(None);
    }
    let changed = sqlx::query(
        "UPDATE users SET password_hash = ?
         WHERE id = ? AND password_hash IS NOT NULL AND disabled = 0 AND role <> 'wall'",
    )
    .bind(password_hash)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .context("nouveau mot de passe")?;
    if changed.rows_affected() != 1 {
        return Ok(None);
    }
    sqlx::query("DELETE FROM auth_sessions WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("fermeture des sessions")?;
    sqlx::query("DELETE FROM password_resets WHERE user_id = ? AND used_at IS NULL")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("retrait des autres liens")?;
    tx.commit().await.context("validation de la réinitialisation")?;
    Ok(Some(user_id))
}

#[cfg(test)]
mod tests;
