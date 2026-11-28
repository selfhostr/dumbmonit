//! La planification des sauvegardes locales, modifiée depuis l'interface, voyage
//! avec le lot : exportée quand elle a été changée, restaurée sur une instance
//! neuve, sans effet quand le lot n'en porte pas.

use dumbmonit_server::backup::local::{self, ScheduleOverride};
use dumbmonit_server::backup::{ExportOptions, collect, restore};
use dumbmonit_server::db;

async fn open(dir: &tempfile::TempDir) -> (sqlx::SqlitePool, dumbmonit_server::crypto::Cipher) {
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("clé");
    (pool, cipher)
}

#[tokio::test]
async fn la_planification_voyage_avec_la_sauvegarde() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, cipher) = open(&source_dir).await;

    // Jamais changée : rien n'est exporté.
    let lot = collect(&source, &cipher, ExportOptions::default()).await.unwrap();
    assert!(lot.backup_schedule.is_none());

    let chosen = ScheduleOverride { enabled: Some(false), interval_hours: Some(6), keep: Some(30) };
    local::save_override(&source, &chosen).await.unwrap();
    let lot = collect(&source, &cipher, ExportOptions::default()).await.unwrap();
    assert!(lot.backup_schedule.is_some());

    let target_dir = tempfile::tempdir().unwrap();
    let (target, target_cipher) = open(&target_dir).await;
    assert_eq!(local::stored_override(&target).await.unwrap(), None);

    let report = restore(&target, &target_cipher, &lot, true).await.unwrap();
    let section = report.sections.iter().find(|s| s.section == "backup_schedule").unwrap();
    assert_eq!((section.created, section.updated, section.skipped), (1, 0, 0));
    assert_eq!(local::stored_override(&target).await.unwrap(), Some(chosen));

    // Relire le même lot ne change rien.
    let again = restore(&target, &target_cipher, &lot, true).await.unwrap();
    let section = again.sections.iter().find(|s| s.section == "backup_schedule").unwrap();
    assert_eq!((section.created, section.updated, section.skipped), (0, 0, 1));
}
