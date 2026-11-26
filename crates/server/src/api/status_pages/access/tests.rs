use super::*;

const KEY: [u8; 32] = [7; 32];

#[test]
fn a_fresh_cookie_is_accepted_until_it_expires() {
    let fp = fingerprint("$argon2id$hash");
    let value = cookie_value(&KEY, 3, 1_000, &fp);
    assert!(cookie_is_valid(&KEY, 3, &fp, &value, 999));
    assert!(!cookie_is_valid(&KEY, 3, &fp, &value, 1_000), "expired");
}

#[test]
fn a_cookie_is_bound_to_its_page_and_its_password() {
    let fp = fingerprint("$argon2id$hash");
    let value = cookie_value(&KEY, 3, 1_000, &fp);
    assert!(!cookie_is_valid(&KEY, 4, &fp, &value, 0), "other page");
    assert!(!cookie_is_valid(&KEY, 3, &fingerprint("$argon2id$other"), &value, 0));
    assert!(!cookie_is_valid(&[8; 32], 3, &fp, &value, 0), "other instance key");
}

#[test]
fn a_forged_or_mangled_cookie_is_refused() {
    let fp = fingerprint("h");
    assert!(!cookie_is_valid(&KEY, 1, &fp, "", 0));
    assert!(!cookie_is_valid(&KEY, 1, &fp, "abc.def", 0));
    assert!(!cookie_is_valid(&KEY, 1, &fp, "99999999999.00", 0));
    // Prolonger l'échéance sans refaire la signature ne passe pas.
    let value = cookie_value(&KEY, 1, 10, &fp);
    let tag = value.split_once('.').unwrap().1;
    assert!(!cookie_is_valid(&KEY, 1, &fp, &format!("99999999999.{tag}"), 0));
}

#[test]
fn the_cookie_is_found_among_others() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_static("a=1; dm_page_3=123.abc; dumbmonit_session=x.y"),
    );
    assert_eq!(read_cookie(&headers, "dm_page_3").as_deref(), Some("123.abc"));
    assert_eq!(read_cookie(&headers, "dm_page_4"), None);
}
