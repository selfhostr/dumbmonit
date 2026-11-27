//! Serveur de courriel de l'instance : le réglage, son mot de passe qui ne
//! revient jamais, et le courriel d'essai, reçu par un faux serveur SMTP local.

use crate::common;

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use common::TestApp;

const PASSWORD: &str = "mot-de-passe-smtp-tres-long";

/// Faux serveur SMTP : accepte tout, rend chaque message reçu (en-têtes compris).
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

fn no_password(body: &Value) {
    assert!(!body.to_string().contains(PASSWORD), "the password came back: {body}");
}

#[tokio::test]
async fn the_email_server_is_saved_without_its_password_ever_coming_back() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;

    let reply = app.get("/api/notify/mail-server", Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["configured"], false);
    assert_eq!(reply.body["has_password"], false);
    assert_eq!(reply.body["locked_by_env"], false);
    assert_eq!(reply.body["security"], "starttls");
    assert_eq!(reply.body["effective_port"], 587);

    // Un lecteur ne voit pas le réglage.
    let viewer = app.viewer_cookie(&admin).await;
    let reply = app.get("/api/notify/mail-server", Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);

    // Une adresse d'expéditeur invalide est refusée.
    let reply = app
        .put(
            "/api/notify/mail-server",
            json!({ "host": "smtp.example.org", "from_address": "not an address" }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);

    let server = json!({
        "host": " smtp.example.org ", "port": 2525, "security": "tls",
        "username": "robot", "from_address": "status@example.org", "from_name": "Acme status",
        "secrets": { "password": PASSWORD }
    });
    let reply = app.put("/api/notify/mail-server", server, Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    no_password(&reply.body);
    assert_eq!(reply.body["host"], "smtp.example.org", "trimmed");
    assert_eq!(reply.body["effective_port"], 2525);
    assert_eq!(reply.body["security"], "tls");
    assert_eq!(reply.body["has_password"], true);
    assert_eq!(reply.body["configured"], true);
    assert!(reply.body.get("password").is_none() && reply.body.get("secrets").is_none());

    let reply = app.get("/api/notify/mail-server", Some(&admin)).await;
    no_password(&reply.body);
    assert_eq!(reply.body["has_password"], true);
    assert_eq!(reply.body["from_name"], "Acme status");

    // Sans `secrets`, le mot de passe est gardé.
    let same = json!({
        "host": "smtp.example.org", "port": 2525, "security": "tls",
        "username": "robot", "from_address": "status@example.org", "from_name": "Ops"
    });
    let reply = app.put("/api/notify/mail-server", same.clone(), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["has_password"], true);
    assert_eq!(reply.body["from_name"], "Ops");

    // Le mot de passe gardé ne suit pas un autre serveur.
    let mut elsewhere = same.clone();
    elsewhere["host"] = json!("smtp.evil.example");
    let reply = app.put("/api/notify/mail-server", elsewhere.clone(), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
    let reply = app
        .post(
            "/api/notify/mail-server/test",
            json!({ "to": "ops@example.org", "server": elsewhere }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
    assert!(reply.body.to_string().contains("password"), "{}", reply.body);

    // `secrets: {}` l'efface.
    let mut cleared = same.clone();
    cleared["secrets"] = json!({});
    let reply = app.put("/api/notify/mail-server", cleared, Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["has_password"], false);

    // Un serveur vide est oublié.
    let reply = app.put("/api/notify/mail-server", json!({}), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["configured"], false);
    assert_eq!(reply.body["host"], "");
}

#[tokio::test]
async fn the_test_email_arrives_or_says_why_it_did_not() {
    let (port, mut mails) = fake_smtp().await;
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;

    // Rien de réglé, rien de joint : rien à essayer.
    let reply = app
        .post("/api/notify/mail-server/test", json!({ "to": "ops@example.org" }), Some(&admin))
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);

    // Le formulaire non enregistré s'essaie tel quel.
    let form = json!({
        "host": "127.0.0.1", "port": port, "security": "none",
        "from_address": "status@example.org", "from_name": "Acme status"
    });
    let reply = app
        .post(
            "/api/notify/mail-server/test",
            json!({ "to": "not an address", "server": form }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
    let reply = app
        .post(
            "/api/notify/mail-server/test",
            json!({ "to": "ops@example.org", "server": form }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["ok"], true);
    let mail = tokio::time::timeout(Duration::from_secs(10), mails.recv())
        .await
        .expect("a mail within 10 s")
        .expect("mail");
    assert!(mail.contains("To: ops@example.org"), "{mail}");
    assert!(mail.contains("Acme status") && mail.contains("status@example.org"), "{mail}");
    assert!(mail.contains("DumbMonit test email"), "{mail}");

    // Enregistré, il s'essaie sans formulaire.
    let reply = app.put("/api/notify/mail-server", form.clone(), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let reply = app
        .post("/api/notify/mail-server/test", json!({ "to": "ops@example.org" }), Some(&admin))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);

    // Un serveur qui ne répond pas : l'erreur SMTP revient, lisible.
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("écoute");
        listener.local_addr().unwrap().port()
    };
    let mut down = form.clone();
    down["port"] = json!(closed);
    let reply = app
        .post(
            "/api/notify/mail-server/test",
            json!({ "to": "ops@example.org", "server": down }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
    assert!(reply.body.to_string().contains("Sending failed"), "{}", reply.body);
}
