//! Boutons interactifs de Slack et de Telegram, et gabarits de message.
//!
//! Le côté sécurité d'abord : sans la signature de la plateforme, rien n'est
//! lu ; avec elle, seul un jeton signé valide agit, une fois, au nom du compte
//! de la messagerie. Puis les gabarits : refusés à l'enregistrement quand une
//! variable est inconnue, rendus par l'aperçu, et emportés par la sauvegarde.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::Utc;
use dumbmonit_server::alerting::group::AlertOutcome;
use dumbmonit_server::alerting::machine::{AlertState, Phase};
use dumbmonit_server::alerting::model::Severity;
use dumbmonit_server::backup::{Bundle, ExportOptions, collect, restore};
use dumbmonit_server::config::Config;
use dumbmonit_server::crypto::Cipher;
use dumbmonit_server::notify::action_link::{self, Action, ActionKey, AlertRef, Claims};
use dumbmonit_server::notify::interactive::telegram;
use dumbmonit_server::notify::message_template::{BODY_KEY, TITLE_KEY};
use dumbmonit_server::state::{AppState, Inner};
use dumbmonit_server::{api, collectors, db, notify, tsdb};
use hmac::{Hmac, KeyInit, Mac};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::SqlitePool;
use tower::ServiceExt;

const UNREACHABLE_VICTORIA: &str = "http://127.0.0.1:1";
const SECRET: &str = "secret-de-test-suffisamment-long";
const PASSWORD: &str = "mot-de-passe-du-homelab";
const SIGNING_SECRET: &str = "slack-signing-secret-de-test";
const BOT_TOKEN: &str = "123456:AAEjetonDeTest";
const HOOK: &str = "https://hooks.slack.com/services/T000/B000/XXXX";

struct TestApp {
    router: Router,
    pool: SqlitePool,
    key: ActionKey,
    cipher: Cipher,
    admin: String,
    _dir: tempfile::TempDir,
}

async fn setup() -> TestApp {
    let dir = tempfile::tempdir().expect("temporary directory");
    let mut config = Config::from_env().expect("default configuration");
    config.data_dir = dir.path().to_path_buf();
    config.setup_code = Some("test-setup-code".into());
    config.victoria_url = Some(UNREACHABLE_VICTORIA.to_string());

    let pool = db::open(&config.database_path()).await.expect("database opened");
    let cipher = db::init_cipher(&pool, SECRET).await.expect("cipher initialised");
    let key = ActionKey::from_cipher(&cipher);
    // Le même secret relu : une seconde clé identique pour le test.
    let test_cipher = db::init_cipher(&pool, SECRET).await.expect("cipher reread");
    db::alerts::seed_builtin_rules(&pool).await.expect("built-in rules seeded");

    let victoria = tsdb::Victoria::new(UNREACHABLE_VICTORIA).expect("client");
    let sink = tsdb::spawn_writer(victoria.clone(), Duration::from_secs(60));
    let mut registry = collectors::Registry::new();
    registry.register(Arc::new(collectors::DummyCollector));
    let state = AppState::new(Inner {
        config,
        pool: pool.clone(),
        cipher,
        victoria,
        sink,
        collectors: registry,
    });

    let mut app = TestApp {
        router: api::router(state),
        pool,
        key,
        cipher: test_cipher,
        admin: String::new(),
        _dir: dir,
    };
    let (status, body) = app
        .json(
            "POST",
            "/api/auth/setup",
            Some(json!({ "setup_code": "test-setup-code", "username": "admin", "password": PASSWORD })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "admin creation: {body}");
    app.admin = app.login().await;
    let (status, body) = app
        .json(
            "PUT",
            "/api/notify/policy",
            Some(json!({ "public_url": "https://monit.example.org/" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    app
}

impl TestApp {
    async fn login(&self) -> String {
        let request = Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "username": "admin", "password": PASSWORD }).to_string()))
            .unwrap();
        let response = self.router.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let raw = response.headers().get(header::SET_COOKIE).expect("session cookie");
        raw.to_str().unwrap().split(';').next().unwrap().to_string()
    }

    /// Requête de l'interface, sous la session de l'administrateur.
    async fn json(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut builder =
            Request::builder().method(method).uri(uri).header("x-requested-with", "DumbMonit");
        if !self.admin.is_empty() {
            builder = builder.header(header::COOKIE, &self.admin);
        }
        let request = match body {
            Some(value) => builder
                .header("content-type", "application/json")
                .body(Body::from(value.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        self.send(request).await
    }

    /// Requête d'une plateforme : ni session ni en-tête anti-CSRF.
    async fn platform(
        &self,
        uri: &str,
        content_type: &str,
        body: String,
        headers: &[(&str, String)],
    ) -> (StatusCode, Value) {
        let mut builder =
            Request::builder().method("POST").uri(uri).header("content-type", content_type);
        for (name, value) in headers {
            builder = builder.header(*name, value);
        }
        self.send(builder.body(Body::from(body)).unwrap()).await
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.router.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn create_channel(&self, body: Value) -> Value {
        let (status, channel) = self.json("POST", "/api/notify/channels", Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{channel}");
        channel
    }

    async fn create_target(&self, name: &str) -> i64 {
        let (status, body) = self
            .json(
                "POST",
                "/api/targets",
                Some(json!({ "name": name, "address": "10.0.0.7", "kind": "dummy" })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "target refused: {body}");
        body["id"].as_i64().expect("target id")
    }

    async fn fire(&self, fingerprint: &str, target_id: i64) {
        let now = Utc::now();
        let outcome = AlertOutcome {
            fingerprint: fingerprint.to_string(),
            rule_uid: "cpu_high".to_string(),
            rule_name: "CPU".to_string(),
            target_id: Some(target_id),
            target_name: "nas".to_string(),
            series_key: format!("dumbmonit_cpu_usage_percent{{host=\"{fingerprint}\"}}"),
            labels: BTreeMap::from([("target".to_string(), target_id.to_string())]),
            state: AlertState {
                phase: Phase::Firing,
                condition_since: Some(now),
                firing_since: Some(now),
                last_eval_at: Some(now),
                last_notified_at: Some(now),
                notify_count: 1,
                value: Some(97.5),
                ..AlertState::default()
            },
            severity: Severity::Warning,
            value: Some(97.5),
            score: None,
            unit: "%".to_string(),
            operator: ">".to_string(),
            threshold: 90.0,
            channels: Vec::new(),
            repeat_interval: Some(Duration::from_secs(3600)),
            escalate_after: None,
            unacked_after: None,
            just_transitioned: false,
            recurred: false,
            detail: None,
        };
        db::alerts::save_states(&self.pool, &[outcome]).await.expect("state saved");
    }

    async fn alert_ref(&self, fingerprint: &str) -> AlertRef {
        let since = db::alerts::get_active(&self.pool, fingerprint)
            .await
            .expect("read")
            .and_then(|alert| alert.state.firing_since);
        AlertRef::new(fingerprint, since)
    }

    async fn token(&self, action: Action, fingerprint: &str) -> String {
        self.key.sign(&Claims::new(action, vec![self.alert_ref(fingerprint).await], Utc::now()))
    }

    async fn alert(&self, fingerprint: &str) -> Value {
        let (status, body) = self.json("GET", "/api/alerts", None).await;
        assert_eq!(status, StatusCode::OK);
        body.as_array()
            .expect("list")
            .iter()
            .find(|alert| alert["fingerprint"] == json!(fingerprint))
            .cloned()
            .unwrap_or(Value::Null)
    }

    async fn audit_count(&self, actor: &str, action: &str) -> usize {
        let (_, audit) = self.json("GET", "/api/auth/audit", None).await;
        audit
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["actor"] == json!(actor) && entry["action"] == json!(action))
            .count()
    }
}

// --------------------------------------------------------------------------
// Slack
// --------------------------------------------------------------------------

fn slack_body(action_id: &str, token: &str) -> String {
    let payload = json!({
        "type": "block_actions",
        "user": {"id": "U024BE7LH", "username": "alice", "name": "alice"},
        "actions": [{"action_id": action_id, "value": token, "type": "button"}],
    });
    format!("payload={}", notify::http::percent_encode(&payload.to_string()))
}

fn slack_headers(secret: &str, timestamp: i64, body: &str) -> Vec<(&'static str, String)> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("v0:{timestamp}:{body}").as_bytes());
    let signature = format!("v0={}", hex::encode(mac.finalize().into_bytes()));
    vec![("x-slack-request-timestamp", timestamp.to_string()), ("x-slack-signature", signature)]
}

async fn slack_channel(app: &TestApp) -> i64 {
    let channel = app
        .create_channel(json!({
            "name": "Slack",
            "kind": "slack",
            "settings": { "interactive": true },
            "secrets": { "webhook_url": HOOK, "signing_secret": SIGNING_SECRET },
        }))
        .await;
    let id = channel["id"].as_i64().unwrap();
    assert_eq!(
        channel["callback_url"],
        json!(format!("https://monit.example.org/api/public/chat/slack/{id}")),
        "the address to paste in the Slack app"
    );
    assert!(channel["settings"].get("signing_secret").is_none(), "never returned");
    id
}

#[tokio::test]
async fn interactive_slack_needs_the_signing_secret() {
    let app = setup().await;
    let (status, body) = app
        .json(
            "POST",
            "/api/notify/channels",
            Some(json!({
                "name": "Slack",
                "kind": "slack",
                "settings": { "interactive": true },
                "secrets": { "webhook_url": HOOK },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("signing_secret"), "{body}");
}

#[tokio::test]
async fn a_signed_slack_press_acknowledges_once_as_the_slack_user() {
    let app = setup().await;
    let id = slack_channel(&app).await;
    let nas = app.create_target("nas").await;
    app.fire("cpu_high@1", nas).await;
    let uri = format!("/api/public/chat/slack/{id}");
    let form = "application/x-www-form-urlencoded";
    let body = slack_body("dumbmonit_ack", &app.token(Action::Ack, "cpu_high@1").await);
    let now = Utc::now().timestamp();

    // Sans signature, avec une fausse, ou trop vieille : refusé, rien n'est fait.
    let (status, _) = app.platform(&uri, form, body.clone(), &[]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let forged = slack_headers("another-secret", now, &body);
    assert_eq!(app.platform(&uri, form, body.clone(), &forged).await.0, StatusCode::FORBIDDEN);
    let stale = slack_headers(SIGNING_SECRET, now - 10 * 60, &body);
    assert_eq!(app.platform(&uri, form, body.clone(), &stale).await.0, StatusCode::FORBIDDEN);
    let tampered = body.replace("alice", "mallory");
    let signed_for_body = slack_headers(SIGNING_SECRET, now, &body);
    let (status, _) = app.platform(&uri, form, tampered, &signed_for_body).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the signature covers the body");
    assert_eq!(app.alert("cpu_high@1").await["acked"], json!(false));

    // Signée : acquittée au nom du compte Slack, et journalisée.
    let headers = slack_headers(SIGNING_SECRET, now, &body);
    let (status, _) = app.platform(&uri, form, body.clone(), &headers).await;
    assert_eq!(status, StatusCode::OK);
    let alert = app.alert("cpu_high@1").await;
    assert_eq!(alert["acked"], json!(true));
    assert_eq!(alert["acked_by"], json!("slack:alice"));
    assert_eq!(app.audit_count("slack:alice", "alert.acked").await, 1);

    // Un second clic (ou une reprise de Slack) ne refait rien.
    let (status, _) = app.platform(&uri, form, body, &headers).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.audit_count("slack:alice", "alert.acked").await, 1, "idempotent");
}

#[tokio::test]
async fn a_slack_press_with_a_forged_or_mismatched_token_does_nothing() {
    let app = setup().await;
    let id = slack_channel(&app).await;
    let nas = app.create_target("nas").await;
    app.fire("cpu_high@1", nas).await;
    let uri = format!("/api/public/chat/slack/{id}");
    let form = "application/x-www-form-urlencoded";
    let now = Utc::now().timestamp();

    // Signé par Slack, mais le jeton du bouton est forgé.
    let other = ActionKey::from_cipher(&Cipher::from_key([1; 32]));
    let forged =
        other.sign(&Claims::new(Action::Ack, vec![app.alert_ref("cpu_high@1").await], Utc::now()));
    for body in [
        slack_body("dumbmonit_ack", &forged),
        slack_body("dumbmonit_ack", "garbage"),
        // Le bouton « Snooze » ne peut pas porter un jeton d'acquittement.
        slack_body("dumbmonit_snooze", &app.token(Action::Ack, "cpu_high@1").await),
        // « Open » est un lien : accusé, sans effet.
        slack_body("dumbmonit_open", ""),
    ] {
        let headers = slack_headers(SIGNING_SECRET, now, &body);
        let (status, _) = app.platform(&uri, form, body, &headers).await;
        assert_eq!(status, StatusCode::OK, "Slack must not retry");
    }
    assert_eq!(app.alert("cpu_high@1").await["acked"], json!(false));

    // Le canal d'un autre type, ou sans boutons, ne reçoit rien.
    let plain = app
        .create_channel(json!({
            "name": "Plain Slack",
            "kind": "slack",
            "secrets": { "webhook_url": HOOK, "signing_secret": SIGNING_SECRET },
        }))
        .await;
    assert!(plain.get("callback_url").is_none());
    let body = slack_body("dumbmonit_ack", &app.token(Action::Ack, "cpu_high@1").await);
    let headers = slack_headers(SIGNING_SECRET, now, &body);
    let uri = format!("/api/public/chat/slack/{}", plain["id"]);
    assert_eq!(app.platform(&uri, form, body, &headers).await.0, StatusCode::FORBIDDEN);
    assert_eq!(app.alert("cpu_high@1").await["acked"], json!(false));
}

// --------------------------------------------------------------------------
// Telegram
// --------------------------------------------------------------------------

#[tokio::test]
async fn a_telegram_press_needs_the_webhook_secret_and_acts_once() {
    let app = setup().await;
    // L'API du bot est injoignable ici : le webhook ne peut pas être posé, et
    // l'échec est consigné sur le canal, sans faire échouer l'enregistrement.
    let channel = app
        .create_channel(json!({
            "name": "Telegram",
            "kind": "telegram",
            "settings": { "chat_id": "-1001", "interactive": true, "api_base": "https://127.0.0.1:1" },
            "secrets": { "bot_token": BOT_TOKEN },
        }))
        .await;
    let id = channel["id"].as_i64().unwrap();
    assert!(
        channel["last_error"].as_str().is_some_and(|e| e.starts_with("Interactive buttons")),
        "{channel}"
    );
    let nas = app.create_target("nas").await;
    app.fire("cpu_high@1", nas).await;

    // Les jetons rangés comme à l'envoi, et leurs références courtes.
    let now = Utc::now();
    let mut message = notify::test_message("x");
    message.actions = action_link::build(
        &app.key,
        "https://monit.example.org",
        &[app.alert_ref("cpu_high@1").await],
        now,
    );
    telegram::remember(&app.pool, &mut message, now).await.expect("stored");
    let (action, reference) = message.context.callbacks[0].clone();
    assert_eq!(action, Action::Ack);
    assert!(reference.len() <= 64);

    let update = json!({
        "update_id": 1,
        "callback_query": {
            "id": "4382bfdwdsb323b2d9",
            "from": {"id": 42, "first_name": "Bob", "username": "bob"},
            "data": reference,
            "message": {"message_id": 9, "chat": {"id": -1001}},
        }
    })
    .to_string();
    let uri = format!("/api/public/chat/telegram/{id}");
    let secret = telegram::webhook_secret(&app.cipher, id, BOT_TOKEN);

    let none: [(&str, String); 0] = [];
    let (status, _) = app.platform(&uri, "application/json", update.clone(), &none).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let wrong = [(telegram::SECRET_HEADER, "0".repeat(64))];
    let (status, _) = app.platform(&uri, "application/json", update.clone(), &wrong).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(app.alert("cpu_high@1").await["acked"], json!(false));

    let good = [(telegram::SECRET_HEADER, secret)];
    let (status, _) = app.platform(&uri, "application/json", update.clone(), &good).await;
    assert_eq!(status, StatusCode::OK);
    let alert = app.alert("cpu_high@1").await;
    assert_eq!(alert["acked_by"], json!("telegram:bob"));
    assert_eq!(app.audit_count("telegram:bob", "alert.acked").await, 1);

    let (status, _) = app.platform(&uri, "application/json", update, &good).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.audit_count("telegram:bob", "alert.acked").await, 1, "idempotent");

    // Une référence inconnue, ou autre chose qu'un clic : accusé, sans effet.
    for other in [
        json!({"update_id": 2, "callback_query": {"id": "1", "from": {"id": 1}, "data": "dm:ffffffffffffffffffffffff"}}),
        json!({"update_id": 3, "message": {"text": "/start"}}),
    ] {
        let (status, _) = app.platform(&uri, "application/json", other.to_string(), &good).await;
        assert_eq!(status, StatusCode::OK);
    }
}

// --------------------------------------------------------------------------
// Gabarits de message
// --------------------------------------------------------------------------

#[tokio::test]
async fn templates_are_checked_on_save_and_previewed() {
    let app = setup().await;
    let (status, body) = app
        .json(
            "POST",
            "/api/notify/channels",
            Some(json!({
                "name": "Bad",
                "kind": "discord",
                "settings": { TITLE_KEY: "{{monitr}} down" },
                "secrets": { "webhook_url": "https://discord.com/api/webhooks/1/x" },
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = body.to_string();
    assert!(text.contains("monitr") && text.contains("monitor"), "{text}");

    let (status, preview) = app
        .json(
            "POST",
            "/api/notify/template-preview",
            Some(json!({ "kind": "slack", "body_template": "*{{monitor}}* <{{url}}|open>" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["format"], json!("slack_mrkdwn"));
    assert_eq!(preview["body"], json!("*nas* <https://monit.example.org/targets/42|open>"));
    assert_eq!(preview["title"], json!("nas — Disk full"), "the default title");

    let (status, error) = app
        .json(
            "POST",
            "/api/notify/template-preview",
            Some(json!({ "kind": "telegram", "body_template": "{{nope}}" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error.to_string().contains("nope"), "{error}");
}

#[tokio::test]
async fn templates_travel_with_the_backup() {
    let app = setup().await;
    let title = "[{{site}}] {{monitor}}";
    let body = "{{summary}}";
    app.create_channel(json!({
        "name": "Templated",
        "kind": "ntfy",
        "settings": { "topic": "homelab", TITLE_KEY: title, BODY_KEY: body },
    }))
    .await;

    let lot = collect(&app.pool, &app.cipher, ExportOptions::default()).await.expect("export");
    let lot: Bundle = serde_json::from_str(&serde_json::to_string(&lot).unwrap()).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.expect("base");
    let other = db::init_cipher(&pool, "une-tout-autre-cle-d-instance-longue").await.expect("clé");
    restore(&pool, &other, &lot, true).await.expect("restore");
    let channels = db::alerts::list_channels(&pool, &other).await.unwrap();
    let restored = channels.iter().find(|c| c.name == "Templated").expect("restored channel");
    assert_eq!(restored.settings[TITLE_KEY], json!(title));
    assert_eq!(restored.settings[BODY_KEY], json!(body));
}
