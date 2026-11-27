//! La note libre d'un moniteur (migration 0263) voyage avec la sauvegarde :
//! elle revient après restauration, et relire le même lot ne change rien.

use dumbmonit_server::backup::{ExportOptions, RestoreReport, collect, restore};
use dumbmonit_server::db;
use sqlx::SqlitePool;

async fn instance(dir: &tempfile::TempDir) -> (SqlitePool, dumbmonit_server::crypto::Cipher) {
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("clé");
    (pool, cipher)
}

fn counts(report: &RestoreReport, name: &str) -> (usize, usize, usize) {
    let section = report.sections.iter().find(|s| s.section == name).expect("section");
    (section.created, section.updated, section.skipped)
}

async fn note(pool: &SqlitePool, address: &str) -> String {
    sqlx::query_scalar("SELECT note FROM targets WHERE address = ?")
        .bind(address)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn la_note_d_un_moniteur_est_sauvegardee_et_restauree() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, cipher) = instance(&source_dir).await;
    sqlx::query(
        "INSERT INTO targets (name, address, kind, interval_secs, enabled, tags, note)
         VALUES ('banc', '10.0.0.2', 'dummy', 60, 1, '{}', 'Ask Sam before a reboot'),
                ('nas', '10.0.0.3', 'dummy', 60, 1, '{}', '')",
    )
    .execute(&source)
    .await
    .unwrap();

    let lot = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    let notes: Vec<(&str, &str)> =
        lot.targets.iter().map(|t| (t.address.as_str(), t.note.as_str())).collect();
    assert!(notes.contains(&("10.0.0.2", "Ask Sam before a reboot")));
    assert!(notes.contains(&("10.0.0.3", "")));

    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&target_dir).await;
    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "targets"), (2, 0, 0), "{report:#?}");
    assert_eq!(note(&pool, "10.0.0.2").await, "Ask Sam before a reboot");
    assert_eq!(note(&pool, "10.0.0.3").await, "");

    let again = restore(&pool, &cipher, &lot, true).await.expect("seconde restauration");
    assert_eq!(counts(&again, "targets"), (0, 0, 2), "rien ne bouge : {again:#?}");

    // Un équipement déjà présent reprend la note du lot.
    sqlx::query("UPDATE targets SET note = 'autre' WHERE address = '10.0.0.2'")
        .execute(&pool)
        .await
        .unwrap();
    let fixed = restore(&pool, &cipher, &lot, true).await.expect("restauration sur l'existant");
    assert_eq!(counts(&fixed, "targets"), (0, 1, 1), "{fixed:#?}");
    assert_eq!(note(&pool, "10.0.0.2").await, "Ask Sam before a reboot");
}
