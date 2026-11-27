//! Sauvegarde du logo téléversé pour les rapports SLA (migration 0247) : il
//! part dans le lot en base64, revient à l'identique, et un fichier que le PDF
//! ne sait pas incorporer est écarté avec une note.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use dumbmonit_server::backup::{Bundle, ExportOptions, collect, restore};
use dumbmonit_server::db;
use dumbmonit_server::reports::logo;
use serde_json::json;

async fn instance() -> (sqlx::SqlitePool, dumbmonit_server::crypto::Cipher, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let pool = db::open(&dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("clé");
    (pool, cipher, dir)
}

/// Un JPEG minimal : SOI, APP0, SOF0 (8 bits, 3×2, 3 composantes), EOI.
fn jpeg() -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
    bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 8, 0, 2, 0, 3, 3]);
    bytes.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1]);
    bytes.extend_from_slice(&[0xFF, 0xD9]);
    bytes
}

#[tokio::test]
async fn the_report_logo_survives_a_backup() {
    let (source, cipher, _dir) = instance().await;
    logo::set_stored(&source, Some(("image/jpeg", jpeg().as_slice()))).await.expect("dépôt");
    let exported = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    let item = exported.report_logo.clone().expect("the logo is in the backup");
    assert_eq!(item.logo_type, "image/jpeg");
    assert_eq!(BASE64.decode(&item.data).expect("base64"), jpeg());

    let (target, cipher, _other) = instance().await;
    let report = restore(&target, &cipher, &exported, true).await.expect("restauration");
    let section = report.sections.iter().find(|s| s.section == "report_logo").expect("section");
    assert_eq!(section.created, 1, "{section:?}");
    let stored = logo::stored(&target).await.expect("lecture").expect("logo");
    assert_eq!(stored, ("image/jpeg".to_string(), jpeg()));

    // Relire le même lot ne change rien.
    let again = restore(&target, &cipher, &exported, true).await.expect("relecture");
    let section = again.sections.iter().find(|s| s.section == "report_logo").expect("section");
    assert_eq!((section.created, section.updated, section.skipped), (0, 0, 1), "{section:?}");
}

#[tokio::test]
async fn a_logo_the_pdf_cannot_embed_is_left_out() {
    let (pool, cipher, _dir) = instance().await;
    logo::set_stored(&pool, Some(("image/jpeg", jpeg().as_slice()))).await.expect("dépôt");
    let webp = BASE64.encode(b"RIFF\0\0\0\0WEBPVP8 ");
    let bundle: Bundle = serde_json::from_value(json!({
        "version": 1,
        "created_at": "2026-11-18 10:00:00",
        "report_logo": { "logo_type": "image/webp", "data": webp }
    }))
    .expect("lot lisible");
    let report = restore(&pool, &cipher, &bundle, true).await.expect("restauration");
    let section = report.sections.iter().find(|s| s.section == "report_logo").expect("section");
    assert_eq!(section.skipped, 1, "{section:?}");
    assert!(!section.notes.is_empty());
    let kept = logo::stored(&pool).await.expect("lecture").expect("logo");
    assert_eq!(kept.1, jpeg(), "the instance keeps its own logo");

    // Un lot sans logo ne touche à rien non plus.
    let empty: Bundle =
        serde_json::from_value(json!({ "version": 1, "created_at": "2026-11-18 10:00:00" }))
            .expect("lot lisible");
    restore(&pool, &cipher, &empty, true).await.expect("restauration");
    assert!(logo::stored(&pool).await.expect("lecture").is_some());
}
