use super::*;

#[test]
fn phone_numbers_are_e164() {
    assert_eq!(normalise_phone("+33 6 12 34 56 78"), Some("+33612345678".into()));
    assert_eq!(normalise_phone("0612345678"), None);
    assert_eq!(normalise_phone("+0123456789"), None);
    assert_eq!(normalise_phone("+33abc"), None);
    assert_eq!(normalise_phone("+1234567"), None);
}

#[test]
fn slack_urls_must_be_incoming_webhooks() {
    assert!(normalise_slack_url("https://hooks.slack.com/services/T0/B0/xyz").is_some());
    assert!(normalise_slack_url("http://hooks.slack.com/services/T0/B0/xyz").is_none());
    assert!(normalise_slack_url("https://evil.example/services/T0/B0/xyz").is_none());
    assert!(normalise_slack_url("https://hooks.slack.com.evil.example/services/a").is_none());
}

#[test]
fn destinations_are_masked_for_admins() {
    assert_eq!(mask("sms", "+33612345678"), "+33•••••••78");
    assert_eq!(
        mask("slack", "https://hooks.slack.com/services/T0/B0/secret"),
        "https://hooks.slack.com/…"
    );
    assert_eq!(mask("email", "a@b.org"), "a@b.org");
}

#[test]
fn signature_is_stable() {
    let a = sign("secret", 1_700_000_000, "{}");
    assert_eq!(a, sign("secret", 1_700_000_000, "{}"));
    assert_ne!(a, sign("secret", 1_700_000_001, "{}"));
    assert!(a.starts_with("sha256=") && a.len() == 7 + 64);
}
