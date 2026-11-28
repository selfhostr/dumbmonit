use super::*;
use crate::notify::action_link::{ActionKey, build};

// Exemple de la documentation de Slack (« Verifying requests from Slack »).
const SECRET: &[u8] = b"8f742231b10e8888abcd99yyyzzz85a5";
const TIMESTAMP: &str = "1531420618";
const BODY: &str = "token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J&team_domain=testteamnow\
    &channel_id=G8PSS9T3V&channel_name=foobar&user_id=U2CERLKJA&user_name=roadrunner\
    &command=%2Fwebhook-collect&text=&response_url=https%3A%2F%2Fhooks.slack.com%2Fcommands\
    %2FT1DC2JH3J%2F397700885554%2F96rGlfmibIGlgcZRskXaIFfN\
    &trigger_id=398738663015.47445629121.803a0bc887a14d10d2c447fce8b6703c";
const SIGNATURE: &str = "v0=a2114d57b48eac39b9ad189dd8316235a7b4a8d21a10bd27519666489c69b503";
const NOW: i64 = 1_531_420_618 + 60;

fn check(secret: &[u8], ts: Option<&str>, sig: Option<&str>, body: &str, now: i64) -> bool {
    verify_signature(secret, ts, sig, body.as_bytes(), now).is_ok()
}

#[test]
fn slack_documented_example_verifies() {
    assert!(check(SECRET, Some(TIMESTAMP), Some(SIGNATURE), BODY, NOW));
}

#[test]
fn any_change_or_a_stale_request_is_refused() {
    let other = b"0000000000000000abcd99yyyzzz85a5";
    assert!(!check(other, Some(TIMESTAMP), Some(SIGNATURE), BODY, NOW), "other secret");
    let tampered = BODY.replace("roadrunner", "coyote");
    assert!(!check(SECRET, Some(TIMESTAMP), Some(SIGNATURE), &tampered, NOW), "body");
    assert!(!check(SECRET, Some("1531420619"), Some(SIGNATURE), BODY, NOW), "timestamp");
    let wrong = SIGNATURE.replace("v0=a2", "v0=b2");
    assert!(!check(SECRET, Some(TIMESTAMP), Some(&wrong), BODY, NOW), "signature");
    assert!(!check(SECRET, Some(TIMESTAMP), Some(&SIGNATURE[3..]), BODY, NOW), "no v0=");
    assert!(!check(SECRET, None, Some(SIGNATURE), BODY, NOW), "no timestamp");
    assert!(!check(SECRET, Some(TIMESTAMP), None, BODY, NOW), "no signature");
    assert!(!check(SECRET, Some("soon"), Some(SIGNATURE), BODY, NOW));
    // Rejeu : six minutes plus tard, la même requête ne passe plus.
    let later = 1_531_420_618 + MAX_SKEW_SECS + 1;
    assert_eq!(
        verify_signature(SECRET, Some(TIMESTAMP), Some(SIGNATURE), BODY.as_bytes(), later),
        Err(SignatureError::Stale)
    );
}

fn message_with_links() -> Message {
    let mut message = crate::notify::message::sample_message(false);
    let key = ActionKey::from_bytes([5; 32]);
    message.actions = build(&key, "https://m.lan", &["a@1".into()], chrono::Utc::now());
    message.link = Some("https://m.lan/targets/42".into());
    message
}

#[test]
fn buttons_carry_the_signed_tokens_and_open_the_device() {
    let message = message_with_links();
    let block = actions_block(&message).expect("buttons");
    let elements = block["elements"].as_array().unwrap();
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[0]["action_id"], "dumbmonit_ack");
    assert_eq!(elements[0]["value"], message.actions[1].token());
    assert_eq!(elements[1]["action_id"], "dumbmonit_snooze");
    assert_eq!(elements[2]["url"], "https://m.lan/targets/42");
    assert_eq!(action_of("dumbmonit_ack"), Some(Action::Ack));
    assert_eq!(action_of("dumbmonit_snooze"), Some(Action::Snooze));
    assert_eq!(action_of("dumbmonit_open"), None);
    assert_eq!(action_of("dumbmonit_dismiss"), None, "not offered as a button");
    let body = payload(&message, block);
    assert_eq!(body["attachments"][0]["blocks"][1]["block_id"], ACTIONS_BLOCK);
    // Sans liens d'action, pas de boutons.
    assert!(actions_block(&crate::notify::message::sample_message(false)).is_none());
}

#[test]
fn a_click_is_read_from_the_form_payload() {
    let payload = json!({
        "type": "block_actions",
        "user": {"id": "U1", "username": "alice", "name": "alice"},
        "actions": [{"action_id": "dumbmonit_ack", "value": "tok.en"}],
        "response_url": "https://hooks.slack.com/actions/T/1/x",
        "message": {"text": "t", "blocks": []},
    });
    let encoded = crate::notify::http::percent_encode(&payload.to_string());
    let body = format!("payload={encoded}");
    let click = parse_interaction(body.as_bytes()).expect("click");
    assert_eq!(click.user, "alice");
    assert_eq!(click.action_id, "dumbmonit_ack");
    assert_eq!(click.value, "tok.en");
    assert!(is_response_url(click.response_url.as_deref().unwrap()));
    assert!(!is_response_url("https://evil.example/hooks.slack.com/"));
    assert_eq!(form_decode("a+b%20c%2B"), Some("a b c+".to_string()));
    assert_eq!(form_decode("%zz"), None);
    assert!(parse_interaction(b"payload=%7B%22type%22%3A%22view_submission%22%7D").is_none());
    assert!(parse_interaction(b"nothing").is_none());
}

#[test]
fn the_updated_message_keeps_open_and_says_who_acted() {
    let message = message_with_links();
    let original = payload(&message, actions_block(&message).unwrap());
    let updated = updated_message(Some(&original), "Acknowledged by alice");
    assert_eq!(updated["replace_original"], true);
    let blocks = updated["attachments"][0]["blocks"].as_array().unwrap();
    assert_eq!(blocks[1]["type"], "context");
    assert_eq!(blocks[1]["elements"][0]["text"], "Acknowledged by alice");
    let left = blocks[2]["elements"].as_array().unwrap();
    assert_eq!(left.len(), 1, "only Open is left");
    assert_eq!(left[0]["action_id"], OPEN_ACTION_ID);
    // Sans le message d'origine, une simple réponse.
    let fallback = updated_message(None, "Acknowledged by alice");
    assert_eq!(fallback["replace_original"], false);
    assert_eq!(fallback["text"], "Acknowledged by alice");
}
