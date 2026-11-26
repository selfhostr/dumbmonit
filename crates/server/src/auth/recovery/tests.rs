use super::*;
use crate::auth::users::Role;

fn user(role: Role, password: bool, email: bool, disabled: bool) -> User {
    User {
        id: 1,
        username: "jane".into(),
        display_name: String::new(),
        role,
        password_hash: password.then(|| "$argon2id$…".to_string()),
        oidc_subject: None,
        oidc_issuer: None,
        created_at: String::new(),
        last_login_at: None,
        disabled,
        totp_secret: None,
        totp_enabled: false,
        ldap_id: None,
        ldap_dn: None,
        disabled_by_ldap: false,
        saml_subject: None,
        saml_issuer: None,
        email: email.then(|| "jane@example.org".to_string()),
        custom_role: None,
    }
}

#[test]
fn only_active_local_password_accounts_can_reset() {
    assert!(can_reset_by_email(&user(Role::Viewer, true, true, false)));
    assert!(can_reset(&user(Role::Admin, true, false, false)));
    assert!(!can_reset_by_email(&user(Role::Admin, true, false, false)), "no address");
    assert!(!can_reset(&user(Role::Viewer, false, true, false)), "SSO or LDAP only");
    assert!(!can_reset(&user(Role::Wall, true, true, false)), "wall display");
    assert!(!can_reset(&user(Role::Operator, true, true, true)), "disabled");
}

#[test]
fn the_settings_have_safe_defaults_and_bounds() {
    let defaults = RecoverySettings::default();
    assert!(defaults.self_service);
    assert_eq!(defaults.reset_minutes, 30);
    assert_eq!(defaults.invite_hours, 168);
    assert!(defaults.validate().is_ok());
    assert!(RecoverySettings { reset_minutes: 2, ..defaults }.validate().is_err());
    assert!(RecoverySettings { reset_minutes: 2000, ..defaults }.validate().is_err());
    assert!(RecoverySettings { invite_hours: 0, ..defaults }.validate().is_err());
    assert!(RecoverySettings { invite_hours: 721, ..defaults }.validate().is_err());
    let partial: RecoverySettings = serde_json::from_str(r#"{"self_service":false}"#).unwrap();
    assert_eq!(partial, RecoverySettings { self_service: false, ..defaults });
}

#[test]
fn only_an_https_origin_serves_as_link_base() {
    let origin = clean_origin("https://monit.lan:8080/");
    assert_eq!(origin.as_deref(), Some("https://monit.lan:8080"));
    assert_eq!(clean_origin("http://monit.lan"), None);
    assert_eq!(clean_origin("https://"), None);
    assert_eq!(clean_origin("https://evil.example/path"), None);
    assert_eq!(clean_origin("https://user@evil.example"), None);
    assert_eq!(clean_origin("https://a b"), None);
}
