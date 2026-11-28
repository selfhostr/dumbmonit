use super::*;
use crate::auth::users::Role;
use axum::http::Method;

fn user(role: Role, totp: bool, password: bool) -> User {
    User {
        id: 1,
        username: "marie".into(),
        display_name: String::new(),
        role,
        password_hash: password.then(|| "$argon2id$…".to_string()),
        oidc_subject: None,
        oidc_issuer: None,
        created_at: "2026-11-22T00:00:00Z".into(),
        last_login_at: None,
        disabled: false,
        totp_secret: None,
        totp_enabled: totp,
        ldap_id: None,
        ldap_dn: None,
        disabled_by_ldap: false,
        saml_subject: None,
        saml_issuer: None,
        email: None,
        custom_role: None,
    }
}

#[test]
fn only_staff_signing_in_by_password_without_a_second_factor_are_concerned() {
    assert!(may_apply(&user(Role::Admin, false, true), Some(Via::Password)));
    assert!(may_apply(&user(Role::Operator, false, true), None));
    assert!(!may_apply(&user(Role::Viewer, false, true), Some(Via::Password)));
    assert!(!may_apply(&user(Role::Wall, false, true), Some(Via::Password)));
    assert!(!may_apply(&user(Role::Wall, false, true), None));
    assert!(!may_apply(&user(Role::Admin, true, true), Some(Via::Password)));
    // Sans mot de passe local (SSO, annuaire) : rien à enrôler ici.
    assert!(!may_apply(&user(Role::Admin, false, false), None));
    for via in [Via::Oidc, Via::Saml, Via::Ldap, Via::Passkey] {
        assert!(!may_apply(&user(Role::Admin, false, true), Some(via)), "{via:?}");
    }
}

#[test]
fn a_blocked_account_can_only_enrol_or_leave() {
    assert!(allowed_while_blocked(&Method::GET, "/api/auth/me"));
    assert!(allowed_while_blocked(&Method::GET, "/auth/totp"));
    assert!(allowed_while_blocked(&Method::POST, "/auth/totp/enroll"));
    assert!(allowed_while_blocked(&Method::POST, "/auth/totp/verify"));
    assert!(allowed_while_blocked(&Method::POST, "/auth/logout"));
    assert!(!allowed_while_blocked(&Method::DELETE, "/auth/totp"));
    assert!(!allowed_while_blocked(&Method::GET, "/targets"));
    assert!(!allowed_while_blocked(&Method::PUT, "/auth/policy"));
    assert!(!allowed_while_blocked(&Method::GET, "/auth/sessions"));
}

#[test]
fn the_policy_is_off_by_default_and_reads_older_shapes() {
    assert!(!Policy::default().require_totp);
    let read: Policy = serde_json::from_str("{}").unwrap();
    assert_eq!(read, Policy::default());
}
