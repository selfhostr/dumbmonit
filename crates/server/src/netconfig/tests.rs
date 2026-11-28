use super::*;

fn cipher() -> Cipher {
    Cipher::from_key([7u8; 32])
}

fn target(tags: &[(&str, &str)]) -> Target {
    Target {
        id: 1,
        name: "sw1".into(),
        address: "192.0.2.1".into(),
        kind: KIND.into(),
        profile_id: None,
        parent_id: None,
        interval: Duration::from_secs(60),
        enabled: true,
        tags: tags.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        credential: dumbmonit_proto::Credential::None,
        group_name: String::new(),
        position: 0,
    }
}

#[test]
fn versions_round_trip_sealed() {
    let cipher = cipher();
    let config = "hostname sw1\nenable secret 5 $1$abc\n";
    let sealed = seal(&cipher, config).unwrap();
    assert!(!sealed.windows(6).any(|w| w == b"$1$abc"), "stored in clear");
    assert_eq!(open(&cipher, &sealed).unwrap(), config);
    // Une autre clé ne lit rien.
    assert!(open(&Cipher::from_key([8u8; 32]), &sealed).is_err());
}

#[test]
fn masking_hides_secrets_but_shows_that_they_changed() {
    let cipher = cipher();
    let before = "hostname sw1\nsnmp-server community public RO\nenable secret 5 $1$old\n";
    let after = "hostname sw1\nsnmp-server community s3cr3t RO\nenable secret 5 $1$old\n";
    let (masked_before, count) = mask(&cipher, "cisco_ios", before);
    let (masked_after, _) = mask(&cipher, "cisco_ios", after);
    assert!(count >= 2);
    for secret in ["public", "s3cr3t", "$1$old"] {
        assert!(!masked_before.contains(secret) && !masked_after.contains(secret), "{secret}");
    }
    assert!(masked_before.contains("hostname sw1"));
    // Le secret changé change d'étiquette, celui qui n'a pas bougé la garde.
    let lines_before: Vec<&str> = masked_before.lines().collect();
    let lines_after: Vec<&str> = masked_after.lines().collect();
    assert_ne!(lines_before[1], lines_after[1]);
    assert_eq!(lines_before[2], lines_after[2]);
}

#[test]
fn an_unknown_profile_still_masks_obvious_secrets() {
    let (masked, count) = mask(&cipher(), "gone", "user admin password hunter2\n");
    assert_eq!(count, 1);
    assert!(!masked.contains("hunter2"));
}

#[test]
fn the_digest_ignores_volatile_lines_and_needs_the_key() {
    let cipher = cipher();
    let profile = fetcher::profile::find("cisco_ios").unwrap();
    let volatile = profile.volatile_rules();
    let a = text::normalize("! Last configuration change at 10:00\nhostname sw1\n", &volatile);
    let b = text::normalize("! Last configuration change at 12:00\nhostname sw1\n", &volatile);
    assert_eq!(digest(&cipher, &a), digest(&cipher, &b));
    assert_ne!(digest(&cipher, &a), digest(&Cipher::from_key([9u8; 32]), &a));
}

#[test]
fn schedule_options_are_bounded() {
    assert_eq!(Schedule::from_target(&target(&[])).unwrap(), Schedule::default());
    assert!(Schedule::from_target(&target(&[("fetch_interval_hours", "0")])).is_err());
    assert!(Schedule::from_target(&target(&[("keep_versions", "5000")])).is_err());
    let custom = Schedule::from_target(&target(&[("fetch_interval_hours", "6")])).unwrap();
    assert_eq!(custom.interval_secs, 6 * 3600);
}

#[test]
fn relay_errors_keep_their_nature() {
    assert_eq!(classify_text("Timed out after 60s"), "down");
    assert_eq!(classify_text("Monitor unreachable: refused"), "down");
    assert_eq!(classify_text("Authentication refused: bad password"), "auth");
    assert_eq!(classify_text("Invalid response: no prompt"), "config");
}
