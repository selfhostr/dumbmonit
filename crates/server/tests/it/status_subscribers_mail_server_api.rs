//! Abonnés par courriel d'une page de statut, envoyés par le serveur de
//! courriel de l'instance, sans aucun canal de notification : l'interrupteur
//! de la page, la confirmation, une annonce et le désabonnement en un clic.
//!
//! Un seul test dans ce binaire : le limiteur du point d'inscription est propre
//! au processus.

use crate::common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower::ServiceExt;

use common::TestApp;

/// Faux serveur SMTP : accepte tout, rend chaque message reçu.
async fn fake_smtp() -> (u16, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("écoute");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            let tx = tx.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                let _ = write.write_all(b"220 fake ESMTP\r\n").await;
                let mut data: Option<String> = None;
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(message) = data.as_mut() {
                        if line == "." {
                            let _ = tx.send(data.take().unwrap_or_default());
                            let _ = write.write_all(b"250 queued\r\n").await;
                        } else {
                            message.push_str(&line);
                            message.push('\n');
                        }
                        continue;
                    }
                    let verb = line.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
                    let reply: &[u8] = match verb.as_str() {
                        "DATA" => {
                            data = Some(String::new());
                            b"354 go ahead\r\n"
                        }
                        "QUIT" => {
                            let _ = write.write_all(b"221 bye\r\n").await;
                            return;
                        }
                        _ => b"250 ok\r\n",
                    };
                    let _ = write.write_all(reply).await;
                }
            });
        }
    });
    (port, rx)
}

async fn next_mail(rx: &mut mpsc::UnboundedReceiver<String>) -> String {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("a mail within 10 s")
        .expect("mail")
}

/// Dé-plie les lignes d'en-tête et le quoted-printable éventuel.
fn readable(mail: &str) -> String {
    mail.replace("=\n", "").replace("=3D", "=").replace("\n ", " ")
}

fn token_after(mail: &str, marker: &str) -> String {
    let text = readable(mail);
    let start = text.find(marker).unwrap_or_else(|| panic!("`{marker}` in mail:\n{text}"));
    text[start + marker.len()..].chars().take_while(|c| c.is_ascii_hexdigit()).collect()
}

async fn anonymous(
    app: &TestApp,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(value) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.router.clone().oneshot(request).await.expect("réponse");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("corps").to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn the_email_server_mails_subscribers_without_any_channel() {
    let (port, mut mails) = fake_smtp().await;
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;

    // Interrupteur ouvert, mais aucun serveur réglé : la page ne propose rien.
    let reply = app
        .post(
            "/api/status-pages",
            json!({ "title": "Relay lab", "slug": "relay-lab", "published": true,
                    "subscribe_email": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    assert_eq!(reply.body["subscribe_email"], true);
    assert!(reply.body["subscribe_channel_id"].is_null());
    let page = reply.body["id"].as_i64().unwrap();
    let (_, doc) = anonymous(&app, "GET", "/api/public/status/relay-lab", None).await;
    assert_eq!(doc["page"]["subscribe"], false, "{doc}");
    let (status, _) = anonymous(
        &app,
        "POST",
        "/api/public/status/relay-lab/subscribe",
        Some(json!({ "email": "reader@example.net" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Le serveur de courriel réglé : le bouton « S'abonner » apparaît.
    let reply = app
        .put(
            "/api/notify/mail-server",
            json!({ "host": "127.0.0.1", "port": port, "security": "none",
                    "from_address": "status@example.org", "from_name": "Relay lab" }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let (_, doc) = anonymous(&app, "GET", "/api/public/status/relay-lab", None).await;
    assert_eq!(doc["page"]["subscribe"], true, "{doc}");
    assert_eq!(doc["page"]["subscribe_kinds"], json!(["email"]));

    // Inscription, confirmation par le lien reçu.
    let (status, body) = anonymous(
        &app,
        "POST",
        "/api/public/status/relay-lab/subscribe",
        Some(json!({ "email": "reader@example.net" })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let mail = next_mail(&mut mails).await;
    assert!(mail.contains("To: reader@example.net"), "{mail}");
    assert!(mail.contains("status@example.org"), "sent from the email server: {mail}");
    let confirm = token_after(&mail, "/s/relay-lab/confirm?token=");
    assert_eq!(confirm.len(), 48);
    let (status, reply) = anonymous(
        &app,
        "POST",
        &format!("/api/public/status/relay-lab/confirm?token={confirm}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");

    // Une annonce part vers l'abonné, avec le désabonnement en un clic.
    let reply = app
        .post(
            "/api/incidents",
            json!({ "title": "Router swap", "page_id": page, "body": "Ten minutes offline." }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let mail = next_mail(&mut mails).await;
    let text = readable(&mail);
    assert!(text.contains("To: reader@example.net"), "{text}");
    assert!(text.contains("Router swap"), "{text}");
    assert!(text.contains("Ten minutes offline."), "{text}");
    assert!(text.contains("List-Unsubscribe-Post: List-Unsubscribe=One-Click"), "{text}");
    let unsubscribe = token_after(&mail, "/api/public/status/relay-lab/unsubscribe?token=");
    assert_eq!(unsubscribe, confirm);

    let (status, _) = anonymous(
        &app,
        "POST",
        &format!("/api/public/status/relay-lab/unsubscribe?token={unsubscribe}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let reply = app.get(&format!("/api/status-pages/{page}/subscribers"), Some(&admin)).await;
    assert_eq!(reply.body, json!([]));

    // L'interrupteur fermé, la page ne propose plus rien ; un PUT qui n'en
    // parle pas le garde.
    let reply = app
        .put(
            &format!("/api/status-pages/{page}"),
            json!({ "title": "Relay lab", "slug": "relay-lab", "published": true,
                    "subscribe_email": false }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["subscribe_email"], false);
    let reply = app
        .put(
            &format!("/api/status-pages/{page}"),
            json!({ "title": "Relay lab", "slug": "relay-lab", "published": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.body["subscribe_email"], false);
    let (_, doc) = anonymous(&app, "GET", "/api/public/status/relay-lab", None).await;
    assert_eq!(doc["page"]["subscribe"], false, "{doc}");
}
