use super::*;

#[test]
fn action_names_cannot_traverse_paths() {
    for good in ["clear-cache", "restart_app", "a", "x1"] {
        assert!(validate_action_name(good).is_ok(), "{good}");
    }
    for bad in [
        "",
        "../etc/passwd",
        "..",
        "a/b",
        "a\\b",
        "/bin/sh",
        "C:evil",
        "-rf",
        ".hidden",
        "Clear",
        "name.yaml",
        "a b",
        "a\0b",
        &"x".repeat(65),
    ] {
        assert!(validate_action_name(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn service_names_cannot_become_options() {
    for good in ["nginx", "nginx.service", "getty@tty1.service", "MSSQL$SQLEXPRESS", "W32Time"] {
        assert!(validate_service_name(good).is_ok(), "{good}");
    }
    for bad in ["", "-H", "--all", "/stop", "a b", "a;b", "a\"b", "../x", "a\nb"] {
        assert!(validate_service_name(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn parameter_values_cannot_inject_arguments() {
    for good in ["high", "eth0", "10.0.0.1", "user@host", "/var/log", "v1+2"] {
        assert!(validate_param_value(good).is_ok(), "{good}");
    }
    for bad in ["", "--force", "-x", "a b", "a;rm", "$(id)", "`id`", "a|b", "a'b", "a\nb"] {
        assert!(validate_param_value(bad).is_err(), "{bad:?}");
    }
    assert!(validate_param_value(&"a".repeat(MAX_PARAM_VALUE_LEN + 1)).is_err());
    assert!(validate_param_key("Level").is_err());
    assert!(validate_param_key("level_2").is_ok());
}

#[test]
fn script_parameters_are_checked_against_the_declared_schema() {
    let script = DeclaredScript {
        name: "flush".into(),
        params: BTreeMap::from([("level".to_string(), vec!["low".into(), "high".into()])]),
        ..DeclaredScript::default()
    };
    let ok = BTreeMap::from([("level".to_string(), "high".to_string())]);
    assert!(script.check_params(&ok).is_ok());
    let wrong = BTreeMap::from([("level".to_string(), "max".to_string())]);
    assert!(script.check_params(&wrong).is_err());
    let extra = BTreeMap::from([
        ("level".to_string(), "low".to_string()),
        ("other".to_string(), "x".to_string()),
    ]);
    assert!(script.check_params(&extra).is_err());
    assert!(script.check_params(&BTreeMap::new()).is_err(), "missing parameter");
}

#[test]
fn a_declaration_is_bounded() {
    let info = RemediationInfo {
        enabled: true,
        services: (0..200).map(|i| format!("svc{i}")).chain(["-bad".to_string()]).collect(),
        scripts: vec![DeclaredScript {
            name: "ok".into(),
            description: "d".repeat(1000),
            params: BTreeMap::from([("k".to_string(), vec!["v".into(), "bad value".into()])]),
            timeout_secs: 99_999,
        }],
    }
    .sanitized();
    assert_eq!(info.services.len(), MAX_DECLARED_SERVICES);
    assert_eq!(info.scripts[0].description.len(), 200);
    assert_eq!(info.scripts[0].params["k"], vec!["v".to_string()]);
    assert_eq!(info.scripts[0].timeout_secs, SCRIPT_MAX_TIMEOUT_SECS);
}

#[test]
fn service_allow_list_ignores_the_unit_suffix() {
    let list = vec!["nginx.service".to_string()];
    assert!(service_allowed(&list, "nginx"));
    assert!(service_allowed(&list, "NGINX.service"));
    assert!(!service_allowed(&list, "sshd"));
    assert!(service_allowed(&["*".to_string()], "anything"));
    assert!(!service_allowed(&[], "nginx"));
}

#[test]
fn nonces_are_32_hex_characters() {
    assert!(validate_nonce("0123456789abcdef0123456789abcdef").is_ok());
    assert!(validate_nonce("short").is_err());
    assert!(validate_nonce(&"z".repeat(32)).is_err());
}
