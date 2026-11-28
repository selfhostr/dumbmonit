//! Sauvegarde des repères de changement (migration 0288) : ils partent dans le
//! lot, reviennent sur une instance neuve (le périmètre « moniteurs » retrouvé
//! par référence), et relire le même lot ne crée rien de plus.

use chrono::Utc;
use dumbmonit_server::annotations::{RawDraft, Scope, store};
use dumbmonit_server::backup::{ExportOptions, collect, restore};
use dumbmonit_server::db;

async fn instance() -> (sqlx::SqlitePool, dumbmonit_server::crypto::Cipher, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("clé");
    (pool, cipher, dir)
}

async fn target(pool: &sqlx::SqlitePool, name: &str, address: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO targets (name, address, kind, group_name) VALUES (?, ?, 'dummy', 'Web')
         RETURNING id",
    )
    .bind(name)
    .bind(address)
    .fetch_one(pool)
    .await
    .expect("équipement")
}

async fn add(pool: &sqlx::SqlitePool, title: &str, at: &str, scope: Scope) {
    let raw = RawDraft {
        title: title.into(),
        at: Some(at.into()),
        until: Some("2026-11-20T10:30:00Z".into()),
        text: "Rolled out".into(),
        link: "https://ci.example/runs/1".into(),
        tags: vec!["deploy".into()],
        scope,
        source: Some("ci".into()),
    };
    let draft = raw.validate(Utc::now(), "api").expect("repère valide");
    store::insert(pool, &draft, "token:ci").await.expect("insertion");
}

#[tokio::test]
async fn markers_survive_a_backup() {
    let (source, cipher, _dir) = instance().await;
    // Un premier équipement décale les identifiants : la restauration doit
    // retrouver le bon par sa référence, pas par son numéro.
    target(&source, "Spare", "10.0.0.9").await;
    let api = target(&source, "API", "10.0.0.2").await;
    add(&source, "Global", "2026-11-20T10:00:00Z", Scope::Global).await;
    add(&source, "API only", "2026-11-20T10:05:00Z", Scope::Monitors { targets: vec![api] }).await;
    add(&source, "Web", "2026-11-20T10:10:00Z", Scope::Folder { folder: "Web".into() }).await;

    let exported = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    assert_eq!(exported.annotations.len(), 3);
    assert_eq!(exported.summary().get("annotations"), Some(&3));

    let (target_pool, cipher, _other) = instance().await;
    let new_api = target(&target_pool, "API", "10.0.0.2").await;
    let report = restore(&target_pool, &cipher, &exported, true).await.expect("restauration");
    let section = report.sections.iter().find(|s| s.section == "annotations").expect("section");
    assert_eq!(section.created, 3, "{section:?}");

    let restored = store::list(&target_pool, None, None, 100).await.expect("lecture");
    assert_eq!(restored.len(), 3);
    let api_only = restored.iter().find(|a| a.title == "API only").expect("repère");
    assert_eq!(api_only.scope, Scope::Monitors { targets: vec![new_api] });
    assert_eq!(api_only.source, "ci");
    assert_eq!(api_only.author, "token:ci");
    assert_eq!(api_only.link, "https://ci.example/runs/1");
    assert_eq!(api_only.until.as_deref(), Some("2026-11-20 10:30:00"));
    assert_eq!(api_only.tags, vec!["deploy".to_string()]);

    // Relire le même lot ne change rien.
    let again = restore(&target_pool, &cipher, &exported, true).await.expect("relecture");
    let section = again.sections.iter().find(|s| s.section == "annotations").expect("section");
    assert_eq!((section.created, section.skipped), (0, 3), "{section:?}");
    assert_eq!(store::count(&target_pool).await.unwrap(), 3);
}

#[tokio::test]
async fn a_marker_whose_monitors_are_gone_is_left_out() {
    let (source, cipher, _dir) = instance().await;
    let api = target(&source, "API", "10.0.0.2").await;
    add(&source, "API only", "2026-11-20T10:05:00Z", Scope::Monitors { targets: vec![api] }).await;
    let mut exported = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    // Un lot où ce moniteur manque (retiré à la main, ou d'une version qui ne
    // l'emportait pas) : le repère qui ne vise que lui est écarté et signalé.
    exported.targets.clear();

    let (target_pool, cipher, _other) = instance().await;
    let report = restore(&target_pool, &cipher, &exported, true).await.expect("restauration");
    let section = report.sections.iter().find(|s| s.section == "annotations").expect("section");
    assert_eq!((section.created, section.skipped), (0, 1), "{section:?}");
    assert!(!section.notes.is_empty());
    assert_eq!(store::count(&target_pool).await.unwrap(), 0);
}
