//! Clics des boutons interactifs de Slack et de Telegram
//! ([`crate::notify::interactive`]).
//!
//! Routes publiques — une plateforme n'a pas de session —, gardées deux fois :
//!
//! - la signature de la plateforme : `X-Slack-Signature` (HMAC du *signing
//!   secret* du canal, fenêtre de rejeu de cinq minutes) ou
//!   `X-Telegram-Bot-Api-Secret-Token` (secret dérivé du secret d'instance,
//!   posé avec le webhook). Sans elle, 403 et rien n'est lu ;
//! - le jeton signé du lien d'action, porté par le bouton : un geste, des
//!   alertes nommées, 24 h, un seul usage. Un second clic, ou une reprise de
//!   la plateforme, ne refait rien et le dit.
//!
//! L'auteur retenu est le compte de la messagerie (`slack:alice`,
//! `telegram:bob`), dans l'acquittement comme dans le journal d'audit.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use chrono::Utc;
use serde_json::Value;

use super::alert_actions::{Applied, apply, links_enabled};
use crate::api::{ApiError, ApiResult};
use crate::db;
use crate::notify::action_link::{Action, ActionKey};
use crate::notify::interactive::{self, done_note, slack, telegram};
use crate::notify::{self, ChannelConfig, DeliveryReport};
use crate::state::AppState;

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/public/chat/slack/{id}", post(slack_interaction))
        .route("/public/chat/telegram/{id}", post(telegram_update))
}

/// Ce qu'un clic a donné, pour la réponse dans la messagerie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Done(Action),
    /// Déjà fait par ce bouton : rien n'est refait.
    Already(Action),
    /// Plus aucune alerte active : rien à faire.
    Inactive,
    /// Jeton refusé (forgé, expiré, autre geste, fonction coupée).
    Refused,
}

/// Le canal `id` s'il est de ce type, actif et en mode interactif. Sinon le
/// même refus qu'une signature fausse : rien à apprendre sur les canaux.
async fn interactive_channel(state: &AppState, id: i64, kind: &str) -> ApiResult<ChannelConfig> {
    let config = db::alerts::get_channel(&state.pool, &state.cipher, id).await?;
    config
        .filter(|config| config.kind == kind && config.enabled && interactive::enabled(config))
        .ok_or_else(refused)
}

/// Vérifie le jeton porté par le bouton et exécute son geste.
///
/// `expected` : le geste que le bouton annonce (Slack le dit à part) ; un
/// jeton d'un autre geste est refusé.
async fn act(
    state: &AppState,
    token: &str,
    expected: Option<Action>,
    actor: &str,
) -> ApiResult<Outcome> {
    let key = ActionKey::from_cipher(&state.cipher);
    let Ok(claims) = key.verify(token, Utc::now()) else { return Ok(Outcome::Refused) };
    if expected.is_some_and(|action| action != claims.action) || !links_enabled(state).await? {
        return Ok(Outcome::Refused);
    }
    Ok(match apply(state, &claims, actor, None).await? {
        Applied::Done(_) => Outcome::Done(claims.action),
        Applied::Used => Outcome::Already(claims.action),
        Applied::Inactive => Outcome::Inactive,
    })
}

/// Phrase rendue à la personne qui a cliqué.
fn reply(outcome: Outcome, who: &str) -> String {
    match outcome {
        Outcome::Done(action) => done_note(action, who),
        Outcome::Already(action) => format!("Already done: {}.", action.label()),
        Outcome::Inactive => "These alerts are no longer active: nothing left to do.".to_string(),
        Outcome::Refused => {
            "This button has expired. Open DumbMonit to act on the alert.".to_string()
        }
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn refused() -> ApiError {
    ApiError::Forbidden("Request signature refused.".into())
}

// --------------------------------------------------------------------------
// Slack
// --------------------------------------------------------------------------

/// `POST /api/public/chat/slack/{id}` : adresse d'interactivité de
/// l'application Slack du canal `id`.
pub async fn slack_interaction(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let config = interactive_channel(&state, id, "slack").await?;
    let Some(secret) = config.secret_opt(slack::SIGNING_SECRET_KEY) else {
        return Err(refused());
    };
    let signed = slack::verify_signature(
        secret.expose().as_bytes(),
        header(&headers, slack::TIMESTAMP_HEADER),
        header(&headers, slack::SIGNATURE_HEADER),
        &body,
        Utc::now().timestamp(),
    );
    if let Err(reason) = signed {
        tracing::debug!(channel = id, ?reason, "Slack request refused");
        return Err(refused());
    }
    // Tout autre événement (« Open », raccourcis) est simplement accusé.
    let Some(click) = slack::parse_interaction(&body) else {
        return Ok(StatusCode::OK.into_response());
    };
    let Some(expected) = slack::action_of(&click.action_id) else {
        return Ok(StatusCode::OK.into_response());
    };
    let actor = interactive::actor("slack", &click.user);
    let outcome = act(&state, &click.value, Some(expected), &actor).await?;
    let note = reply(outcome, &click.user);
    // La réponse doit partir en moins de trois secondes : la mise à jour du
    // message suit, à part.
    if let Some(url) = click.response_url.filter(|url| slack::is_response_url(url))
        && !notify::sending_disabled()
    {
        let update = match outcome {
            Outcome::Done(_) | Outcome::Already(_) => {
                slack::updated_message(click.message.as_ref(), &note)
            }
            _ => serde_json::json!({
                "replace_original": false,
                "response_type": "ephemeral",
                "text": note,
            }),
        };
        tokio::spawn(async move {
            let http = notify::http_client();
            let request = notify::http::Request::post(&url).json(update).secret_text(url.clone());
            if let Err(error) = notify::http::send(&http, request).await {
                tracing::warn!(%error, "Slack message not updated");
            }
        });
    }
    Ok(StatusCode::OK.into_response())
}

// --------------------------------------------------------------------------
// Telegram
// --------------------------------------------------------------------------

/// `POST /api/public/chat/telegram/{id}` : webhook du bot du canal `id`,
/// posé par [`sync_telegram_webhook`].
pub async fn telegram_update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let config = interactive_channel(&state, id, "telegram").await?;
    let token = config.secret("bot_token").map_err(|_| refused())?;
    let expected = telegram::webhook_secret(&state.cipher, id, token.expose());
    if !telegram::secret_matches(&expected, header(&headers, telegram::SECRET_HEADER)) {
        tracing::debug!(channel = id, "Telegram request refused");
        return Err(refused());
    }
    // Telegram réessaie tant qu'il n'a pas un 2xx : tout ce qui ne se lit pas
    // est accusé et ignoré.
    let update: Value = crate::untrusted::from_slice(&body).unwrap_or(Value::Null);
    let Some(callback) = telegram::parse_update(&update) else {
        return Ok(StatusCode::OK.into_response());
    };
    let now = Utc::now();
    let outcome = if !telegram::is_reference(&callback.data) {
        None
    } else {
        match telegram::recall(&state.pool, &callback.data, now).await? {
            Some(signed) => {
                let actor = interactive::actor("telegram", &callback.user);
                Some(act(&state, &signed, None, &actor).await?)
            }
            None => Some(Outcome::Refused),
        }
    };
    if notify::sending_disabled() {
        return Ok(StatusCode::OK.into_response());
    }
    let note = outcome.map(|outcome| reply(outcome, &callback.user)).unwrap_or_default();
    tokio::spawn(async move {
        let http = notify::http_client();
        if let Err(error) = telegram::answer(&http, &config, &callback.id, &note).await {
            tracing::warn!(%error, "Telegram button not answered");
        }
        let (Some(chat), Some(message)) = (callback.chat_id, callback.message_id) else { return };
        if matches!(outcome, Some(Outcome::Done(_) | Outcome::Already(_))) {
            let keyboard = telegram::done_keyboard(callback.keyboard.as_ref(), &note);
            let edited = telegram::edit_keyboard(&http, &config, chat, message, keyboard).await;
            if let Err(error) = edited {
                tracing::warn!(%error, "Telegram message not updated");
            }
        }
    });
    Ok(StatusCode::OK.into_response())
}

/// Pose ou retire le webhook Telegram d'un canal qui vient d'être enregistré.
/// Un échec ne fait pas échouer l'enregistrement : il est consigné comme
/// dernière erreur du canal, que l'interface affiche à côté de lui.
pub(crate) async fn sync_telegram_webhook(state: &AppState, id: i64, was_interactive: bool) {
    let Ok(Some(config)) = db::alerts::get_channel(&state.pool, &state.cipher, id).await else {
        return;
    };
    if config.kind != "telegram"
        || (!interactive::enabled(&config) && !was_interactive)
        || notify::sending_disabled()
    {
        return;
    }
    let env_url = crate::config::env_var("DUMBMONIT_PUBLIC_URL");
    let public_url = match crate::notify::policy_store::load_global(&state.pool).await {
        Ok(global) => global.public_url(env_url.as_deref()),
        Err(_) => None,
    };
    let http = notify::http_client();
    let synced = telegram::sync_webhook(
        &http,
        &state.cipher,
        &config,
        was_interactive,
        public_url.as_deref(),
    )
    .await;
    if let Err(error) = synced {
        let report = DeliveryReport {
            channel_id: id,
            channel_name: config.name.clone(),
            error: Some(format!("Interactive buttons: {error}")),
            transient: error.is_transient(),
        };
        if let Err(error) = db::alerts::record_delivery(&state.pool, &report, Utc::now()).await {
            tracing::warn!(?error, "webhook error not recorded");
        }
    }
}

/// Retire le webhook d'un canal Telegram interactif supprimé.
pub(crate) fn forget_telegram_webhook(config: ChannelConfig) {
    if config.kind != "telegram" || !interactive::enabled(&config) || notify::sending_disabled() {
        return;
    }
    tokio::spawn(async move {
        let http = notify::http_client();
        if let Err(error) = telegram::delete_webhook(&http, &config).await {
            tracing::warn!(%error, "Telegram webhook not removed");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reply_says_who_did_what_or_why_nothing_happened() {
        assert_eq!(reply(Outcome::Done(Action::Ack), "alice"), "Acknowledged by alice");
        assert_eq!(reply(Outcome::Done(Action::Snooze), "bob"), "Snoozed for 1 h by bob");
        assert_eq!(reply(Outcome::Already(Action::Ack), "alice"), "Already done: Acknowledge.");
        assert!(reply(Outcome::Refused, "x").contains("expired"));
        assert!(reply(Outcome::Inactive, "x").contains("no longer active"));
    }
}
