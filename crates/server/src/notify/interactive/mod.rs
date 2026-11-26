//! Boutons interactifs des messageries : acquitter ou mettre en pause une
//! alerte depuis le message lui-même, sans ouvrir DumbMonit.
//!
//! Trois services, trois mécaniques :
//!
//! - **Slack** : boutons Block Kit. Un clic part vers l'adresse
//!   d'interactivité de l'application Slack (`/api/public/chat/slack/{id}`),
//!   signée par le *signing secret* de l'application ([`slack`]).
//! - **Telegram** : clavier en ligne à `callback_data`. Le clic arrive par le
//!   webhook du bot, posé à l'enregistrement du canal, avec son en-tête secret
//!   ([`telegram`]).
//! - **Microsoft Teams** : un webhook de flux « Workflows » ne renvoie rien à
//!   l'émetteur (ni `Action.Submit` ni `Action.Execute` sans bot, et
//!   `Action.Http` ne vaut que pour Outlook). Les boutons sont donc des
//!   `Action.OpenUrl` vers les liens d'action signés, qui demandent une
//!   confirmation — c'est le mieux que ces webhooks permettent.
//!
//! Dans les trois cas, l'autorisation est le jeton signé des liens d'action
//! ([`crate::notify::action_link`]) : un geste, des alertes nommées, 24 h, un
//! seul usage. La signature de la plateforme ne fait que prouver que la
//! requête vient d'elle ; elle n'autorise rien à elle seule.

pub mod slack;
pub mod telegram;

use serde_json::{Value, json};

use crate::notify::action_link::Action;
use crate::notify::channel::ChannelConfig;
use crate::notify::message::Message;

/// Réglage booléen « Interactive buttons » d'un canal Slack, Telegram ou Teams.
pub const INTERACTIVE_KEY: &str = "interactive";

/// Les gestes proposés en boutons, dans l'ordre : « Open » s'y ajoute quand le
/// message a un lien.
pub const BUTTON_ACTIONS: [Action; 2] = [Action::Ack, Action::Snooze];

/// Libellé du bouton qui ouvre l'équipement dans DumbMonit.
pub const OPEN_LABEL: &str = "Open";

/// Longueur maximale du nom d'utilisateur retenu comme auteur.
const MAX_ACTOR_LEN: usize = 64;

/// Vrai si le canal a demandé des boutons interactifs.
pub fn enabled(config: &ChannelConfig) -> bool {
    config.setting_bool(INTERACTIVE_KEY, false)
}

/// Chemin public qui reçoit les clics d'un canal.
pub fn callback_path(kind: &str, channel_id: i64) -> String {
    format!("/api/public/chat/{kind}/{channel_id}")
}

/// Adresse complète à donner à la plateforme, `None` sans adresse publique
/// ou pour un type de canal qui ne rappelle pas (Teams).
pub fn callback_url(config: &ChannelConfig, public_url: Option<&str>) -> Option<String> {
    callback_url_for(&config.kind, config.id, enabled(config), public_url)
}

/// Comme [`callback_url`], depuis les champs d'un canal (résumé de l'API).
pub fn callback_url_for(
    kind: &str,
    channel_id: i64,
    interactive: bool,
    public_url: Option<&str>,
) -> Option<String> {
    if !interactive || !matches!(kind, "slack" | "telegram") {
        return None;
    }
    let base = public_url?.trim_end_matches('/');
    (!base.is_empty()).then(|| format!("{base}{}", callback_path(kind, channel_id)))
}

/// Auteur d'un geste fait depuis une messagerie, tel que l'acquittement et le
/// journal d'audit le retiennent : `slack:alice`, `telegram:bob`. Le nom vient
/// de la plateforme ; il est borné et débarrassé des caractères de contrôle.
pub fn actor(platform: &str, user: &str) -> String {
    let user: String = user
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_ACTOR_LEN)
        .collect::<String>()
        .trim()
        .to_string();
    let user = if user.is_empty() { "unknown".to_string() } else { user };
    format!("{platform}:{user}")
}

/// Phrase qui remplace les boutons une fois le geste fait.
pub fn done_note(action: Action, who: &str) -> String {
    match action {
        Action::Ack => format!("Acknowledged by {who}"),
        Action::Snooze => format!("Snoozed for 1 h by {who}"),
        Action::Dismiss => format!("Dismissed by {who}"),
    }
}

/// Boutons d'une carte Teams : des `Action.OpenUrl` vers les liens signés
/// (voir l'en-tête du module), plus « Open ». Vide sans liens d'action.
pub fn teams_actions(message: &Message) -> Vec<Value> {
    let mut actions: Vec<Value> = BUTTON_ACTIONS
        .iter()
        .filter_map(|action| message.actions.iter().find(|link| link.action == *action))
        .map(|link| json!({"type": "Action.OpenUrl", "title": link.label, "url": link.url}))
        .collect();
    if actions.is_empty() {
        return actions;
    }
    if let Some(link) = &message.link {
        actions.push(json!({"type": "Action.OpenUrl", "title": OPEN_LABEL, "url": link}));
    }
    actions
}

#[cfg(test)]
mod tests;
