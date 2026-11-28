//! Telegram : clavier en ligne à `callback_data`, reçu par le webhook du bot.
//!
//! Un `callback_data` tient en 64 octets, bien moins qu'un jeton signé : le
//! bouton porte donc une référence courte (`dm:` + l'aléa du jeton, en
//! hexadécimal), et le jeton lui-même est rangé en base le temps de sa
//! validité (`chat_action_tokens`). Au clic, la référence redonne le jeton,
//! qui est vérifié exactement comme celui d'un lien.
//!
//! Le webhook est posé par [`sync_webhook`] à l'enregistrement du canal, avec
//! un `secret_token` dérivé du secret d'instance, que Telegram renvoie dans
//! [`SECRET_HEADER`] à chaque appel. Sans adresse publique en `https`,
//! Telegram refuse le webhook : le message garde alors ses boutons-liens.

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use subtle::ConstantTimeEq;

use super::{BUTTON_ACTIONS, OPEN_LABEL};
use crate::crypto::Cipher;
use crate::notify::action_link::{self, Action, TTL};
use crate::notify::channel::ChannelConfig;
use crate::notify::error::NotifyError;
use crate::notify::http::{self, Request};
use crate::notify::message::Message;
use crate::notify::message_template::{TextFormat, escape};
use crate::notify::secret::SecretString;

/// En-tête par lequel Telegram prouve que l'appel vient du webhook posé.
pub const SECRET_HEADER: &str = "x-telegram-bot-api-secret-token";
/// Préfixe des références de bouton.
pub const CALLBACK_PREFIX: &str = "dm:";
/// `callback_data` du bouton d'état laissé après le geste : il ne fait rien.
pub const DONE_DATA: &str = "dm:done";
/// Sous-clé du secret de webhook : aucun autre usage du secret d'instance ne
/// peut produire la même valeur.
const WEBHOOK_LABEL: &[u8] = b"dumbmonit/telegram-webhook/v1";
const DEFAULT_API: &str = "https://api.telegram.org";

/// Secret du webhook d'un canal : lié au canal et à son bot, si bien qu'un
/// changement de jeton change aussi le secret (le webhook est reposé).
pub fn webhook_secret(cipher: &Cipher, channel_id: i64, bot_token: &str) -> String {
    let data = format!("{channel_id}\n{bot_token}");
    hex::encode(cipher.keyed_digest(WEBHOOK_LABEL, data.as_bytes()))
}

/// Compare l'en-tête reçu au secret attendu, en temps constant.
pub fn secret_matches(expected: &str, received: Option<&str>) -> bool {
    let Some(received) = received else { return false };
    !expected.is_empty() && bool::from(expected.as_bytes().ct_eq(received.trim().as_bytes()))
}

/// Adresse acceptée par Telegram dans un bouton : `https`, nom de domaine
/// avec un point, ni IP ni `localhost`.
pub fn button_url_ok(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else { return false };
    let Some(host) = parsed.domain() else { return false };
    parsed.scheme() == "https" && host.contains('.') && !host.ends_with(".localhost")
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Range les jetons des boutons de geste et pose leurs références courtes
/// dans `message.context.callbacks`. Sans liens d'action utilisables en
/// bouton, ne fait rien : le message garde ses boutons-liens.
pub async fn remember(
    pool: &SqlitePool,
    message: &mut Message,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    let links: Vec<_> = BUTTON_ACTIONS
        .iter()
        .filter_map(|action| message.actions.iter().find(|link| link.action == *action))
        .collect();
    if links.len() != BUTTON_ACTIONS.len() || !links.iter().all(|link| button_url_ok(&link.url)) {
        return Ok(());
    }
    sqlx::query("DELETE FROM chat_action_tokens WHERE expires_at < ?")
        .bind(stamp(now))
        .execute(pool)
        .await?;
    let mut callbacks = Vec::new();
    for link in links {
        let token = link.token();
        let Some(nonce) = action_link::nonce_hex_unverified(token) else { return Ok(()) };
        sqlx::query(
            "INSERT INTO chat_action_tokens (nonce, token, expires_at) VALUES (?, ?, ?)
             ON CONFLICT(nonce) DO NOTHING",
        )
        .bind(&nonce)
        .bind(token)
        .bind(stamp(now + TTL))
        .execute(pool)
        .await?;
        callbacks.push((link.action, format!("{CALLBACK_PREFIX}{nonce}")));
    }
    message.context.callbacks = callbacks;
    Ok(())
}

/// Le jeton d'une référence de bouton, s'il est encore rangé.
pub async fn recall(
    pool: &SqlitePool,
    reference: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<Option<String>> {
    let Some(nonce) = reference.strip_prefix(CALLBACK_PREFIX) else { return Ok(None) };
    if nonce.len() != 24 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    sqlx::query_scalar("SELECT token FROM chat_action_tokens WHERE nonce = ? AND expires_at >= ?")
        .bind(nonce)
        .bind(stamp(now))
        .fetch_optional(pool)
        .await
}

/// Clavier interactif du message, `None` sans références enregistrées.
pub fn keyboard(message: &Message) -> Option<Value> {
    let mut row = Vec::new();
    for action in BUTTON_ACTIONS {
        let (_, reference) =
            message.context.callbacks.iter().find(|(callback, _)| *callback == action)?;
        row.push(json!({"text": action.label(), "callback_data": reference}));
    }
    if let Some(link) = message.link.as_deref().filter(|link| button_url_ok(link)) {
        row.push(json!({"text": OPEN_LABEL, "url": link}));
    }
    Some(json!({"inline_keyboard": [row]}))
}

/// Texte MarkdownV2 d'un message rendu par un gabarit Telegram, `None` sinon.
/// `links` : ajouter les liens d'action dans le texte (pas de clavier).
pub fn markdown_v2_text(message: &Message, links: bool) -> Option<String> {
    let rendered = message.context.rendered.as_ref()?;
    if rendered.format != TextFormat::TelegramMarkdownV2 {
        return None;
    }
    let mut out = format!("*{}*", rendered.title);
    if !rendered.body.is_empty() {
        out.push('\n');
        out.push_str(&rendered.body);
    }
    if links && !message.actions.is_empty() {
        let md = |text: &str, url: bool| escape(TextFormat::TelegramMarkdownV2, text, url);
        let links: Vec<String> = message
            .actions
            .iter()
            .map(|link| format!("[{}]({})", md(link.label, false), md(&link.url, true)))
            .collect();
        out.push('\n');
        out.push_str(&links.join(" · "));
    }
    Some(out)
}

/// Un clic reçu par le webhook.
#[derive(Debug, Clone, PartialEq)]
pub struct Callback {
    pub id: String,
    /// `username` de la personne, à défaut son prénom, à défaut son identifiant.
    pub user: String,
    pub data: String,
    pub chat_id: Option<i64>,
    pub message_id: Option<i64>,
    /// Clavier du message d'origine, pour le récrire.
    pub keyboard: Option<Value>,
}

/// Lit un `callback_query` d'une mise à jour Telegram ; `None` pour tout le
/// reste (messages, éditions), que le webhook ignore.
pub fn parse_update(update: &Value) -> Option<Callback> {
    let query = update.get("callback_query")?;
    let from = &query["from"];
    let user = from["username"]
        .as_str()
        .or_else(|| from["first_name"].as_str())
        .map(str::to_string)
        .or_else(|| from["id"].as_i64().map(|id| id.to_string()))?;
    let message = &query["message"];
    Some(Callback {
        id: query["id"].as_str()?.to_string(),
        user,
        data: query["data"].as_str().unwrap_or_default().to_string(),
        chat_id: message["chat"]["id"].as_i64(),
        message_id: message["message_id"].as_i64(),
        keyboard: message.get("reply_markup").filter(|markup| markup.is_object()).cloned(),
    })
}

/// Le clavier après le geste : un bouton d'état (« Acknowledged by bob »),
/// puis les boutons-liens d'origine (« Open »), qui servent encore.
pub fn done_keyboard(original: Option<&Value>, note: &str) -> Value {
    let links: Vec<Value> = original
        .and_then(|markup| markup["inline_keyboard"].as_array())
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
        .filter(|button| button.get("url").is_some())
        .cloned()
        .collect();
    let mut rows = vec![json!([{"text": note, "callback_data": DONE_DATA}])];
    if !links.is_empty() {
        rows.push(Value::Array(links));
    }
    json!({"inline_keyboard": rows})
}

// --------------------------------------------------------------------------
// Appels à l'API des bots
// --------------------------------------------------------------------------

/// Adresse d'une méthode de l'API du bot de ce canal, et son jeton.
fn method_url(config: &ChannelConfig, method: &str) -> Result<(String, SecretString), NotifyError> {
    let token = config.secret("bot_token")?;
    let api = config.setting_opt("api_base").unwrap_or_else(|| DEFAULT_API.to_string());
    http::check_url(&api, "api_base")?;
    Ok((format!("{}/bot{}/{method}", api.trim_end_matches('/'), token.expose()), token))
}

fn explain(status: u16, body: &str) -> Option<String> {
    Some(
        match status {
            400 if body.contains("HTTPS") || body.contains("webhook") => {
                "Telegram refused the webhook for interactive buttons: it needs the instance's \
                 public URL in https, on port 443, 80, 88 or 8443"
            }
            400 if body.contains("query is too old") => "this button is older than Telegram allows",
            401 => "Telegram bot token invalid",
            _ => return None,
        }
        .to_string(),
    )
}

async fn call(
    http: &reqwest::Client,
    config: &ChannelConfig,
    method: &str,
    body: Value,
) -> Result<(), NotifyError> {
    let (url, token) = method_url(config, method)?;
    http::send(
        http,
        Request::post(&url).json(body).secret(&token).secret_text(url.clone()).explain(explain),
    )
    .await
}

/// Pose (ou retire) le webhook du bot selon le réglage du canal.
///
/// `was_interactive` : l'état d'avant l'enregistrement. Le webhook n'est
/// retiré que s'il avait été posé par DumbMonit : un bot qui sert aussi à
/// autre chose garde le sien tant que la fonction n'a jamais été activée.
pub async fn sync_webhook(
    http: &reqwest::Client,
    cipher: &Cipher,
    config: &ChannelConfig,
    was_interactive: bool,
    public_url: Option<&str>,
) -> Result<(), NotifyError> {
    if config.kind != "telegram" {
        return Ok(());
    }
    if !super::enabled(config) {
        if was_interactive {
            return delete_webhook(http, config).await;
        }
        return Ok(());
    }
    let Some(url) = super::callback_url(config, public_url).filter(|url| button_url_ok(url)) else {
        return Err(NotifyError::Config(
            "interactive buttons need the instance's public URL in https with a domain name; \
             until then, messages keep their link buttons"
                .to_string(),
        ));
    };
    let token = config.secret("bot_token")?;
    let secret = webhook_secret(cipher, config.id, token.expose());
    let body = json!({
        "url": url,
        "secret_token": secret,
        "allowed_updates": ["callback_query"],
    });
    call(http, config, "setWebhook", body).await
}

/// Retire le webhook du bot (canal supprimé, ou boutons désactivés).
pub async fn delete_webhook(
    http: &reqwest::Client,
    config: &ChannelConfig,
) -> Result<(), NotifyError> {
    call(http, config, "deleteWebhook", json!({})).await
}

/// Répond au clic (le petit message en haut de l'écran).
pub async fn answer(
    http: &reqwest::Client,
    config: &ChannelConfig,
    callback_id: &str,
    text: &str,
) -> Result<(), NotifyError> {
    let body = json!({"callback_query_id": callback_id, "text": text});
    call(http, config, "answerCallbackQuery", body).await
}

/// Remplace le clavier du message d'origine.
pub async fn edit_keyboard(
    http: &reqwest::Client,
    config: &ChannelConfig,
    chat_id: i64,
    message_id: i64,
    keyboard: Value,
) -> Result<(), NotifyError> {
    let body = json!({"chat_id": chat_id, "message_id": message_id, "reply_markup": keyboard});
    call(http, config, "editMessageReplyMarkup", body).await
}

/// Le geste d'une référence ne se lit qu'en base : cette fonction dit
/// seulement si les données d'un clic ressemblent à une référence.
pub fn is_reference(data: &str) -> bool {
    data.starts_with(CALLBACK_PREFIX) && data != DONE_DATA
}

/// Le message Telegram court d'un geste refusé ou déjà fait.
pub fn refusal(already: bool, action: Option<Action>) -> &'static str {
    match (already, action) {
        (true, Some(Action::Snooze)) => "Already snoozed.",
        (true, _) => "Already done.",
        (false, _) => "This button has expired. Open DumbMonit to act on the alert.",
    }
}

#[cfg(test)]
mod tests;
