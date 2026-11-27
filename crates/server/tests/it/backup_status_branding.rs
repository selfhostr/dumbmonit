//! La marque blanche d'une page de statut (« Hide DumbMonit branding ») voyage
//! avec la sauvegarde ; une ancienne sauvegarde, qui n'en dit rien, laisse le
//! réglage de la page tel qu'il est.

use dumbmonit_server::backup::{Bundle, ExportOptions, RestoreReport, collect, restore};
use dumbmonit_server::crypto::Cipher;
use dumbmonit_server::db;
use serde_json::json;
use sqlx::SqlitePool;

async fn instance(dir: &tempfile::TempDir) -> (SqlitePool, Cipher) {
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("clé");
    (pool, cipher)
}

fn counts(report: &RestoreReport, name: &str) -> (usize, usize, usize) {
    let section = report.sections.iter().find(|s| s.section == name).expect("section");
    (section.created, section.updated, section.skipped)
}

async fn hidden(pool: &SqlitePool, slug: &str) -> bool {
    let value: i64 = sqlx::query_scalar("SELECT hide_branding FROM status_pages WHERE slug = ?")
        .bind(slug)
        .fetch_one(pool)
        .await
        .unwrap();
    value != 0
}

#[tokio::test]
async fn la_marque_blanche_est_sauvegardee_et_restauree() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, cipher) = instance(&source_dir).await;
    sqlx::query(
        "INSERT INTO status_pages (slug, title, published, hide_branding)
         VALUES ('acme', 'Acme', 1, 1), ('lab', 'Lab', 1, 0)",
    )
    .execute(&source)
    .await
    .unwrap();

    let lot = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    let acme = lot.status_pages.iter().find(|p| p.slug == "acme").unwrap();
    assert_eq!(acme.hide_branding, Some(true));
    // Passage par le JSON, comme un vrai fichier.
    let lot: Bundle = serde_json::from_str(&serde_json::to_string(&lot).unwrap()).unwrap();

    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&target_dir).await;
    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "status_pages"), (2, 0, 0), "{report:#?}");
    assert!(hidden(&pool, "acme").await);
    assert!(!hidden(&pool, "lab").await);

    // Rien ne bouge à la seconde restauration.
    let again = restore(&pool, &cipher, &lot, true).await.expect("seconde restauration");
    assert_eq!(counts(&again, "status_pages"), (0, 0, 2), "{again:#?}");

    // Le réglage changé ici est remis à la valeur du fichier.
    sqlx::query("UPDATE status_pages SET hide_branding = 0 WHERE slug = 'acme'")
        .execute(&pool)
        .await
        .unwrap();
    let back = restore(&pool, &cipher, &lot, true).await.expect("troisième restauration");
    assert_eq!(counts(&back, "status_pages"), (0, 1, 1), "{back:#?}");
    assert!(hidden(&pool, "acme").await);
}

#[tokio::test]
async fn une_ancienne_sauvegarde_garde_le_reglage_en_place() {
    let old: Bundle = serde_json::from_value(json!({
        "version": 1,
        "created_at": "2026-09-01 10:00:00",
        "source_version": "0.17.0",
        "status_pages": [
            { "slug": "vieille", "title": "Vieille", "published": true, "theme": "auto",
              "show_uptime_days": 30 }
        ]
    }))
    .expect("ancien lot lisible");
    assert_eq!(old.status_pages[0].hide_branding, None);

    let dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&dir).await;
    restore(&pool, &cipher, &old, true).await.expect("restauration");
    assert!(!hidden(&pool, "vieille").await, "défaut : la marque reste");

    sqlx::query("UPDATE status_pages SET hide_branding = 1 WHERE slug = 'vieille'")
        .execute(&pool)
        .await
        .unwrap();
    let report = restore(&pool, &cipher, &old, true).await.expect("seconde restauration");
    assert_eq!(counts(&report, "status_pages"), (0, 0, 1), "{report:#?}");
    assert!(hidden(&pool, "vieille").await, "le fichier n'en dit rien : réglage gardé");
}
