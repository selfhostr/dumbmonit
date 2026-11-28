use super::*;

#[test]
fn a_token_round_trips_through_its_link_form() {
    let token = LinkToken::generate();
    let value = token.value();
    let parsed = LinkToken::parse(&value).expect("forme valide");
    assert_eq!(parsed.id(), token.id());
    assert!(parsed.matches(&token.secret_hash()));
    // Recopié en majuscules (lecture à voix haute, client de courriel) : le même.
    let upper = LinkToken::parse(&value.to_uppercase()).expect("majuscules");
    assert!(upper.matches(&token.secret_hash()));
}

#[test]
fn two_tokens_never_share_a_secret_and_the_secret_is_long() {
    let a = LinkToken::generate();
    let b = LinkToken::generate();
    assert_ne!(a.id(), b.id());
    assert!(!a.matches(&b.secret_hash()));
    let value = a.value();
    let secret = value.split_once('.').unwrap().1;
    assert!(secret.len() * 4 >= 256, "au moins 256 bits de secret");
}

#[test]
fn malformed_tokens_are_refused() {
    let good = LinkToken::generate().value();
    let (id, secret) = good.split_once('.').unwrap();
    for bad in [
        String::new(),
        id.to_string(),
        format!("{id}."),
        format!(".{secret}"),
        format!("{id}.{}", &secret[1..]),
        format!("{id}.{}z", &secret[1..]),
        format!("{id}x.{secret}"),
    ] {
        assert!(LinkToken::parse(&bad).is_none(), "{bad}");
    }
}

#[test]
fn debug_never_prints_the_secret() {
    let token = LinkToken::generate();
    let rendered = format!("{token:?}");
    let value = token.value();
    let secret = value.split_once('.').unwrap().1;
    assert!(!rendered.contains(secret), "{rendered}");
    assert!(rendered.contains("<redacted>"));
}
