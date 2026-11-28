use super::*;
use crate::notify::smtp::Smtp;
use std::collections::HashMap;

fn server() -> MailServer {
    MailServer {
        host: "smtp.example.org".into(),
        port: None,
        security: MailSecurity::StartTls,
        username: "dumbmonit".into(),
        from_address: "status@example.org".into(),
        from_name: "Acme status".into(),
    }
}

#[test]
fn the_default_port_follows_the_encryption() {
    let mut s = server();
    assert_eq!(s.effective_port(), 587);
    s.security = MailSecurity::Tls;
    assert_eq!(s.effective_port(), 465);
    s.security = MailSecurity::Plain;
    assert_eq!(s.effective_port(), 25);
    s.port = Some(2525);
    assert_eq!(s.effective_port(), 2525);
}

#[test]
fn the_security_is_written_as_the_channels_write_it() {
    assert_eq!(serde_json::to_value(MailSecurity::Plain).unwrap(), json!("none"));
    assert_eq!(serde_json::to_value(MailSecurity::Tls).unwrap(), json!("tls"));
    let parsed: MailServer = serde_json::from_value(json!({"host": "h"})).unwrap();
    assert_eq!(parsed.security, MailSecurity::StartTls, "missing fields take their default");
}

#[test]
fn a_saved_entry_is_checked() {
    assert!(MailServer::default().validate().is_ok(), "empty means no server");
    assert!(server().validate().is_ok());
    let mut s = server();
    s.from_address = "not an address".into();
    assert!(s.validate().unwrap_err().contains("sender"));
    let mut s = server();
    s.host = "smtp://smtp.example.org".into();
    assert!(s.validate().is_err());
    let mut s = server();
    s.host = String::new();
    assert!(s.validate().unwrap_err().contains("host"));
    let mut s = server();
    s.port = Some(0);
    assert!(s.validate().is_err());
    assert!(check_password("p\nq").is_err());
}

#[test]
fn the_server_becomes_a_sending_channel_without_recipient() {
    let channel = server().channel(Some("s3cret"));
    assert_eq!(channel.kind, "smtp");
    assert_eq!(channel.settings["port"], 587);
    assert_eq!(channel.settings["security"], "starttls");
    assert_eq!(channel.settings["from"], "status@example.org");
    assert_eq!(channel.settings["from_name"], "Acme status");
    assert_eq!(channel.secrets["password"], "s3cret");
    assert!(Smtp::sender(&channel).is_ok());
    let anonymous = MailServer { username: String::new(), ..server() }.channel(None);
    assert!(anonymous.settings.get("username").is_none());
    assert_eq!(anonymous.secrets, json!({}));
}

#[test]
fn the_environment_describes_the_same_server() {
    let env: HashMap<&str, &str> = HashMap::from([
        ("DUMBMONIT_SMTP_HOST", " mail.example.org "),
        ("DUMBMONIT_SMTP_PORT", "2525"),
        ("DUMBMONIT_SMTP_SECURITY", "TLS"),
        ("DUMBMONIT_SMTP_USERNAME", "robot"),
        ("DUMBMONIT_SMTP_PASSWORD", "pw"),
        ("DUMBMONIT_SMTP_FROM", "robot@example.org"),
        ("DUMBMONIT_SMTP_FROM_NAME", "Robot"),
    ]);
    let (server, password) =
        from_env_with(|key| env.get(key).map(|v| v.to_string())).expect("env server");
    assert_eq!(server.host, "mail.example.org");
    assert_eq!(server.port, Some(2525));
    assert_eq!(server.security, MailSecurity::Tls);
    assert_eq!(server.username, "robot");
    assert_eq!(server.from_address, "robot@example.org");
    assert_eq!(server.from_name, "Robot");
    assert_eq!(password.as_deref(), Some("pw"));
}

#[test]
fn without_a_host_the_environment_says_nothing() {
    let env: HashMap<&str, &str> = HashMap::from([("DUMBMONIT_SMTP_FROM", "a@b.org")]);
    assert!(from_env_with(|key| env.get(key).map(|v| v.to_string())).is_none());
    let env: HashMap<&str, &str> =
        HashMap::from([("DUMBMONIT_SMTP_HOST", "h"), ("DUMBMONIT_SMTP_PORT", "nope")]);
    let (server, _) = from_env_with(|key| env.get(key).map(|v| v.to_string())).unwrap();
    assert_eq!(server.port, None, "an unreadable port falls back to the default");
}
