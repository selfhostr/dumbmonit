use super::*;
use crate::notify::action_link::{ActionKey, build};
use crate::notify::message::sample_message;

#[test]
fn the_webhook_secret_is_per_channel_and_bot_and_checked_in_constant_time() {
    let cipher = Cipher::from_key([9; 32]);
    let secret = webhook_secret(&cipher, 1, "123:abc");
    assert_eq!(secret.len(), 64);
    assert!(secret.bytes().all(|b| b.is_ascii_hexdigit()), "allowed by Telegram");
    assert_ne!(secret, webhook_secret(&cipher, 2, "123:abc"));
    assert_ne!(secret, webhook_secret(&cipher, 1, "123:other"));
    assert!(secret_matches(&secret, Some(&secret)));
    assert!(!secret_matches(&secret, Some("nope")));
    assert!(!secret_matches(&secret, None));
    assert!(!secret_matches("", Some("")));
}

#[test]
fn the_keyboard_uses_short_references_and_keeps_open() {
    let mut message = sample_message(false);
    assert!(keyboard(&message).is_none(), "no references, no interactive keyboard");
    message.link = Some("https://monit.example.org/targets/42".into());
    message.context.callbacks = vec![
        (Action::Ack, format!("{CALLBACK_PREFIX}{}", "a".repeat(24))),
        (Action::Snooze, format!("{CALLBACK_PREFIX}{}", "b".repeat(24))),
    ];
    let markup = keyboard(&message).expect("keyboard");
    let row = markup["inline_keyboard"][0].as_array().unwrap();
    assert_eq!(row.len(), 3);
    assert_eq!(row[0]["text"], "Acknowledge");
    assert!(row[0]["callback_data"].as_str().unwrap().len() <= 64, "Telegram's limit");
    assert_eq!(row[2]["url"], "https://monit.example.org/targets/42");
}

#[test]
fn a_callback_query_is_read_and_anything_else_ignored() {
    let update = json!({
        "update_id": 1,
        "callback_query": {
            "id": "77",
            "from": {"id": 42, "first_name": "Bob", "username": "bob"},
            "data": "dm:0123456789abcdef01234567",
            "message": {
                "message_id": 9,
                "chat": {"id": -1001},
                "reply_markup": {"inline_keyboard": [[
                    {"text": "Acknowledge", "callback_data": "dm:x"},
                    {"text": "Open", "url": "https://m.example.org/targets/1"}
                ]]}
            }
        }
    });
    let callback = parse_update(&update).expect("callback");
    assert_eq!(callback.user, "bob");
    assert_eq!(callback.chat_id, Some(-1001));
    assert_eq!(callback.message_id, Some(9));
    assert!(is_reference(&callback.data));
    assert!(!is_reference(DONE_DATA));
    assert!(parse_update(&json!({"update_id": 2, "message": {"text": "hi"}})).is_none());

    let after = done_keyboard(callback.keyboard.as_ref(), "Acknowledged by bob");
    assert_eq!(after["inline_keyboard"][0][0]["text"], "Acknowledged by bob");
    assert_eq!(after["inline_keyboard"][0][0]["callback_data"], DONE_DATA);
    assert_eq!(after["inline_keyboard"][1][0]["text"], "Open");
    assert_eq!(after["inline_keyboard"].as_array().unwrap().len(), 2);
}

#[test]
fn only_https_domains_make_buttons() {
    assert!(button_url_ok("https://monit.example.org/x"));
    assert!(!button_url_ok("http://monit.example.org/x"));
    assert!(!button_url_ok("https://192.168.1.2/x"));
    assert!(!button_url_ok("https://monit/x"));
}

#[test]
fn a_templated_message_is_sent_in_markdown_v2_with_escaped_links() {
    use crate::notify::message_template::{MessageTemplates, templated};
    let mut message = sample_message(false);
    let key = ActionKey::from_bytes([4; 32]);
    message.actions = build(&key, "https://m.lan", &["a@1".into()], Utc::now());
    assert!(markdown_v2_text(&message, true).is_none(), "no template, the usual HTML");
    let templates = MessageTemplates::parse(Some("{{monitor}}!"), Some("{{rule}}.")).unwrap();
    let message = templated(&templates.unwrap(), &message, TextFormat::TelegramMarkdownV2);
    let text = markdown_v2_text(&message, true).expect("markdown");
    assert!(text.starts_with("*nas\\!*\nDisk full\\.\n[Dismiss](https://m.lan/alert-action#"));
    assert!(!markdown_v2_text(&message, false).unwrap().contains("alert-action"));
}
