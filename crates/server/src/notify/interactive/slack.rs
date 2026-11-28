//! Slack : boutons Block Kit et vérification des clics.
//!
//! Un clic sur un bouton est posté par Slack vers l'adresse d'interactivité
//! de l'application (*Interactivity & Shortcuts → Request URL*), en
//! `application/x-www-form-urlencoded` avec un champ `payload` JSON. La
//! requête est signée : `X-Slack-Signature = "v0=" hex(HMAC-SHA256(signing
//! secret, "v0:" horodatage ":" corps))`, avec `X-Slack-Request-Timestamp`.
//! Une requête de plus de [`MAX_SKEW_SECS`] est refusée : un clic capturé ne
//! se rejoue pas.

use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use super::{BUTTON_ACTIONS, OPEN_LABEL};
use crate::notify::action_link::Action;
use crate::notify::message::Message;

/// Secret du canal : le *signing secret* de l'application Slack.
pub const SIGNING_SECRET_KEY: &str = "signing_secret";
pub const SIGNATURE_HEADER: &str = "x-slack-signature";
pub const TIMESTAMP_HEADER: &str = "x-slack-request-timestamp";
/// Fenêtre de rejeu, celle que Slack recommande.
pub const MAX_SKEW_SECS: i64 = 5 * 60;
/// Identifiant du bloc de boutons, retiré du message une fois le geste fait.
pub const ACTIONS_BLOCK: &str = "dumbmonit_actions";
/// Taille maximale de la valeur d'un bouton chez Slack.
const MAX_VALUE_LEN: usize = 2000;
const OPEN_ACTION_ID: &str = "dumbmonit_open";

/// Pourquoi une requête est refusée (journal de débogage seulement).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureError {
    Missing,
    Stale,
    Mismatch,
}

/// Vérifie la signature d'une requête de Slack, en temps constant.
pub fn verify_signature(
    signing_secret: &[u8],
    timestamp: Option<&str>,
    signature: Option<&str>,
    body: &[u8],
    now_unix: i64,
) -> Result<(), SignatureError> {
    let (Some(timestamp), Some(signature)) = (timestamp, signature) else {
        return Err(SignatureError::Missing);
    };
    let timestamp = timestamp.trim();
    let at: i64 = timestamp.parse().map_err(|_| SignatureError::Missing)?;
    if (now_unix - at).abs() > MAX_SKEW_SECS {
        return Err(SignatureError::Stale);
    }
    let Some(hex_tag) = signature.trim().strip_prefix("v0=") else {
        return Err(SignatureError::Mismatch);
    };
    let tag = hex::decode(hex_tag).map_err(|_| SignatureError::Mismatch)?;
    let mut mac =
        Hmac::<Sha256>::new_from_slice(signing_secret).expect("HMAC accepts any key length");
    mac.update(b"v0:");
    mac.update(timestamp.as_bytes());
    mac.update(b":");
    mac.update(body);
    let expected = mac.finalize().into_bytes();
    if bool::from(expected.as_slice().ct_eq(tag.as_slice())) {
        Ok(())
    } else {
        Err(SignatureError::Mismatch)
    }
}

/// `action_id` d'un bouton de geste.
fn action_id(action: Action) -> String {
    format!("dumbmonit_{}", action.as_str())
}

/// Le geste d'un `action_id` reçu, `None` pour « Open » ou un inconnu.
pub fn action_of(action_id: &str) -> Option<Action> {
    BUTTON_ACTIONS.into_iter().find(|action| self::action_id(*action) == action_id)
}

/// Bloc de boutons du message, `None` quand le message n'a pas de liens
/// d'action (pas d'adresse publique, avalanche) : le canal retombe alors sur
/// ses liens habituels.
pub fn actions_block(message: &Message) -> Option<Value> {
    let mut elements = Vec::new();
    for action in BUTTON_ACTIONS {
        let link = message.actions.iter().find(|link| link.action == action)?;
        let token = link.token();
        if token.is_empty() || token.len() > MAX_VALUE_LEN {
            return None;
        }
        let mut button = json!({
            "type": "button",
            "action_id": action_id(action),
            "text": {"type": "plain_text", "text": link.label},
            "value": token,
        });
        if action == Action::Ack {
            button["style"] = json!("primary");
        }
        elements.push(button);
    }
    if let Some(link) = &message.link {
        elements.push(json!({
            "type": "button",
            "action_id": OPEN_ACTION_ID,
            "text": {"type": "plain_text", "text": OPEN_LABEL},
            "url": link,
        }));
    }
    Some(json!({"type": "actions", "block_id": ACTIONS_BLOCK, "elements": elements}))
}

/// Corps d'un message à boutons : la pièce jointe colorée habituelle, son
/// contenu en blocs (texte, puis boutons).
pub fn payload(message: &Message, actions: Value) -> Value {
    json!({
        "text": message.title,
        "attachments": [{
            "color": format!("#{:06x}", message.color()),
            "fallback": message.title,
            "blocks": [
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!("*{}*\n{}", message.title, message.text),
                    },
                },
                actions,
            ],
        }],
    })
}

/// Un clic reçu, réduit à ce qui sert.
#[derive(Debug, Clone, PartialEq)]
pub struct Interaction {
    /// Nom d'utilisateur Slack (à défaut, son identifiant).
    pub user: String,
    pub action_id: String,
    pub value: String,
    pub response_url: Option<String>,
    /// Le message d'origine, pour le récrire sans ses boutons.
    pub message: Option<Value>,
}

/// Décode une valeur `application/x-www-form-urlencoded`.
fn form_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = raw.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// Lit le clic d'un corps de requête Slack (`payload=…`). `None` pour tout
/// autre événement que `block_actions` ou un corps illisible.
pub fn parse_interaction(body: &[u8]) -> Option<Interaction> {
    let body = std::str::from_utf8(body).ok()?;
    let raw = body.split('&').find_map(|pair| pair.strip_prefix("payload="))?;
    let payload: Value = serde_json::from_str(&form_decode(raw)?).ok()?;
    if payload["type"] != "block_actions" {
        return None;
    }
    let action = payload["actions"].as_array()?.first()?;
    let user = &payload["user"];
    let name = ["username", "name", "id"]
        .iter()
        .find_map(|key| user[*key].as_str().filter(|value| !value.is_empty()))?;
    Some(Interaction {
        user: name.to_string(),
        action_id: action["action_id"].as_str()?.to_string(),
        value: action["value"].as_str().unwrap_or_default().to_string(),
        response_url: payload["response_url"].as_str().map(str::to_string),
        message: payload.get("message").filter(|m| m.is_object()).cloned(),
    })
}

/// Vrai pour une adresse de réponse de Slack : la seule où le serveur
/// accepte de poster la mise à jour du message.
pub fn is_response_url(url: &str) -> bool {
    url.starts_with("https://hooks.slack.com/")
}

/// Retire le bloc de boutons d'une liste de blocs et y ajoute la note. Vrai
/// si le bloc y était.
fn replace_actions(blocks: &mut Vec<Value>, note: &str) -> bool {
    let Some(index) = blocks.iter().position(|block| block["block_id"] == ACTIONS_BLOCK) else {
        return false;
    };
    // « Open » reste : c'est un lien, il sert encore.
    let open: Vec<Value> = blocks[index]["elements"]
        .as_array()
        .map(|elements| elements.iter().filter(|e| e.get("url").is_some()).cloned().collect())
        .unwrap_or_default();
    let context = json!({"type": "context", "elements": [{"type": "mrkdwn", "text": note}]});
    if open.is_empty() {
        blocks[index] = context;
    } else {
        blocks[index] = json!({"type": "actions", "block_id": ACTIONS_BLOCK, "elements": open});
        blocks.insert(index, context);
    }
    true
}

/// Le message d'origine récrit : les boutons de geste remplacés par la note
/// (« Acknowledged by @alice »), à poster sur `response_url`.
pub fn updated_message(original: Option<&Value>, note: &str) -> Value {
    let mut message = original.cloned().unwrap_or_else(|| json!({}));
    let mut done = false;
    if let Some(blocks) = message.get_mut("blocks").and_then(Value::as_array_mut) {
        done |= replace_actions(blocks, note);
    }
    if let Some(attachments) = message.get_mut("attachments").and_then(Value::as_array_mut) {
        for attachment in attachments {
            if let Some(blocks) = attachment.get_mut("blocks").and_then(Value::as_array_mut) {
                done |= replace_actions(blocks, note);
            }
        }
    }
    let mut out = json!({
        "replace_original": true,
        "text": message.get("text").cloned().unwrap_or_else(|| json!(note)),
    });
    if let Some(blocks) = message.get("blocks") {
        out["blocks"] = blocks.clone();
    }
    if let Some(attachments) = message.get("attachments") {
        out["attachments"] = attachments.clone();
    }
    if !done {
        // Message d'origine introuvable : au moins dire ce qui a été fait.
        out = json!({"replace_original": false, "response_type": "in_channel", "text": note});
    }
    out
}

#[cfg(test)]
mod tests;
