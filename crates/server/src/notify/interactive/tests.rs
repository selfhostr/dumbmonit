use super::*;
use crate::notify::action_link::{ActionKey, build};
use serde_json::json;

fn config(kind: &str, interactive: bool) -> ChannelConfig {
    ChannelConfig {
        id: 7,
        name: "chat".into(),
        kind: kind.into(),
        enabled: true,
        settings: json!({ INTERACTIVE_KEY: interactive }),
        secrets: json!({}),
    }
}

#[test]
fn the_actor_names_the_platform_and_a_clean_user() {
    assert_eq!(actor("slack", "alice"), "slack:alice");
    assert_eq!(actor("telegram", " bob\n\u{7}"), "telegram:bob");
    assert_eq!(actor("slack", ""), "slack:unknown");
    assert_eq!(actor("slack", &"x".repeat(500)).len(), "slack:".len() + MAX_ACTOR_LEN);
}

#[test]
fn only_slack_and_telegram_call_back_and_only_with_a_public_url() {
    assert_eq!(
        callback_url(&config("slack", true), Some("https://m.lan/")).as_deref(),
        Some("https://m.lan/api/public/chat/slack/7")
    );
    assert!(callback_url(&config("slack", false), Some("https://m.lan")).is_none());
    assert!(callback_url(&config("teams", true), Some("https://m.lan")).is_none());
    assert!(callback_url(&config("telegram", true), None).is_none());
}

#[test]
fn teams_buttons_open_the_signed_links() {
    let key = ActionKey::from_bytes([3; 32]);
    let mut message = crate::notify::message::sample_message(false);
    assert!(teams_actions(&message).is_empty(), "no links, no buttons");
    message.actions = build(&key, "https://m.lan", &["a@1".into()], chrono::Utc::now());
    message.link = Some("https://m.lan/targets/42".into());
    let actions = teams_actions(&message);
    let titles: Vec<&str> = actions.iter().filter_map(|a| a["title"].as_str()).collect();
    assert_eq!(titles, ["Acknowledge", "Snooze 1 h", "Open"]);
    assert!(actions.iter().all(|a| a["type"] == "Action.OpenUrl"));
    assert!(actions[0]["url"].as_str().unwrap().starts_with("https://m.lan/alert-action#"));
}
