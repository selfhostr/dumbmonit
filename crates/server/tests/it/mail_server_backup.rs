//! Le serveur de courriel de l'instance voyage avec la sauvegarde, son mot de
//! passe en clair dans le lot (chiffré par la phrase de passe) puis rechiffré
//! par la clé de l'instance restaurée ; l'interrupteur d'abonnement par
//! courriel des pages de statut aussi.

use dumbmonit_server::backup::{Bundle, ExportOptions, collect, restore};
use dumbmonit_server::crypto::Cipher;
use dumbmonit_server::db;
use dumbmonit_server::notify::mail_server::{
    self, MailSecurity, MailServer, PASSWORD_KEY, PasswordChange,
};
use sqlx::SqlitePool;

const PASSWORD: &str = "mot-de-passe-smtp-tres-long";

async fn instance(dir: &tempfile::TempDir, secret: &str) -> (SqlitePool, Cipher) {
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&pool, secret).await.expect("clé");
    (pool, cipher)
}

fn server() -> MailServer {
    MailServer {
        host: "smtp.example.org".into(),
        port: Some(2525),
        security: MailSecurity::Tls,
        username: "robot".into(),
        from_address: "status@example.org".into(),
        from_name: "Acme status".into(),
    }
}

async fn sealed_password(pool: &SqlitePool) -> Vec<u8> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(PASSWORD_KEY)
        .fetch_one(pool)
        .await
        .expect("stored password")
}

#[tokio::test]
async fn the_email_server_and_its_password_are_backed_up_and_restored() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, cipher) = instance(&source_dir, "secret-de-test-suffisamment-long").await;
    mail_server::save(&source, &cipher, &server(), PasswordChange::Set(PASSWORD.into()))
        .await
        .expect("save");
    // Chiffré au repos.
    let sealed = sealed_password(&source).await;
    assert!(!String::from_utf8_lossy(&sealed).contains(PASSWORD));
    sqlx::query(
        "INSERT INTO status_pages (slug, title, published, subscribe_email)
         VALUES ('acme', 'Acme', 1, 1)",
    )
    .execute(&source)
    .await
    .unwrap();

    let lot = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    let saved = lot.mail_server.clone().expect("mail server in the bundle");
    assert_eq!(saved.server, server());
    assert_eq!(saved.password.as_deref(), Some(PASSWORD));
    assert_eq!(lot.status_pages[0].subscriptions.as_ref().and_then(|s| s.email), Some(true));
    let json = serde_json::to_string(&lot).unwrap();
    let lot: Bundle = serde_json::from_str(&json).unwrap();

    // Une autre instance, une autre clé.
    let target_dir = tempfile::tempdir().unwrap();
    let (pool, other) = instance(&target_dir, "une-tout-autre-cle-d-instance-longue").await;
    let report = restore(&pool, &other, &lot, true).await.expect("restauration");
    let section = report.sections.iter().find(|s| s.section == "mail_server").expect("section");
    assert_eq!(section.created, 1, "{:?}", section.notes);
    let restored = mail_server::load_stored(&pool).await.unwrap().expect("restored");
    assert_eq!(restored, server());
    let password = mail_server::load_password(&pool, &other).await.unwrap();
    assert_eq!(password.as_deref(), Some(PASSWORD));
    assert!(!String::from_utf8_lossy(&sealed_password(&pool).await).contains(PASSWORD));
    let open: i64 =
        sqlx::query_scalar("SELECT subscribe_email FROM status_pages WHERE slug = 'acme'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(open, 1);

    // Restaurée une seconde fois, rien ne change.
    let report = restore(&pool, &other, &lot, true).await.expect("restauration");
    let section = report.sections.iter().find(|s| s.section == "mail_server").expect("section");
    assert_eq!((section.created, section.updated, section.skipped), (0, 0, 1));
}

#[tokio::test]
async fn an_old_bundle_keeps_the_email_server_and_opens_email_for_a_named_channel() {
    let dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&dir, "secret-de-test-suffisamment-long").await;
    mail_server::save(&pool, &cipher, &server(), PasswordChange::Set(PASSWORD.into()))
        .await
        .expect("save");
    sqlx::query(
        "INSERT INTO notification_channels (name, kind, enabled, settings, secret_enc)
         VALUES ('Mail', 'smtp', 1, '{\"host\":\"h\",\"from\":\"a@b.org\",\"to\":[\"c@d.org\"]}', NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO status_pages (slug, title, published) VALUES ('old', 'Old', 1)")
        .execute(&pool)
        .await
        .unwrap();

    // Un lot d'avant le serveur de courriel et l'interrupteur.
    let lot: Bundle = serde_json::from_value(serde_json::json!({
        "version": 1,
        "created_at": "2026-11-01 10:00:00",
        "status_pages": [
            { "slug": "old", "title": "Old", "published": true, "theme": "auto",
              "show_uptime_days": 90, "subscriptions": { "email_channel": "Mail" } }
        ]
    }))
    .expect("old bundle");
    assert!(lot.mail_server.is_none());
    restore(&pool, &cipher, &lot, true).await.expect("restauration");

    assert_eq!(mail_server::load_stored(&pool).await.unwrap(), Some(server()), "untouched");
    let password = mail_server::load_password(&pool, &cipher).await.unwrap();
    assert_eq!(password.as_deref(), Some(PASSWORD));
    let (open, channel): (i64, Option<i64>) = sqlx::query_as(
        "SELECT subscribe_email, subscribe_channel_id FROM status_pages WHERE slug = 'old'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(open, 1);
    assert!(channel.is_some());
}
