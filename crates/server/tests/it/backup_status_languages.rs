//! La langue par défaut d'une page de statut et ses traductions voyagent avec la
//! sauvegarde ; une ancienne sauvegarde, qui n'en dit rien, les laisse en place.

use dumbmonit_server::backup::{Bundle, ExportOptions, RestoreReport, collect, restore};
use dumbmonit_server::crypto::Cipher;
use dumbmonit_server::db;
use dumbmonit_server::db::status_lang::{self, Translation};
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

async fn page_id(pool: &SqlitePool, slug: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM status_pages WHERE slug = ?")
        .bind(slug)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn french() -> Translation {
    Translation {
        lang: "fr".into(),
        title: "État du service".into(),
        description: "Toutes les nouvelles".into(),
        footer_text: String::new(),
        labels: [("API".to_string(), "Interface".to_string())].into(),
    }
}

#[tokio::test]
async fn les_langues_d_une_page_sont_sauvegardees_et_restaurees() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, cipher) = instance(&source_dir).await;
    sqlx::query("INSERT INTO status_pages (slug, title, published) VALUES ('acme', 'Acme', 1)")
        .execute(&source)
        .await
        .unwrap();
    let id = page_id(&source, "acme").await;
    status_lang::replace(&source, id, "de", &[french()]).await.unwrap();

    let lot = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    let acme = lot.status_pages.iter().find(|p| p.slug == "acme").unwrap();
    assert_eq!(acme.default_lang.as_deref(), Some("de"));
    assert_eq!(acme.translations, vec![french()]);
    // Passage par le JSON, comme un vrai fichier.
    let lot: Bundle = serde_json::from_str(&serde_json::to_string(&lot).unwrap()).unwrap();

    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&target_dir).await;
    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "status_pages"), (1, 0, 0), "{report:#?}");
    let restored = page_id(&pool, "acme").await;
    assert_eq!(status_lang::default_lang(&pool, restored).await.unwrap(), "de");
    assert_eq!(status_lang::list(&pool, restored).await.unwrap(), vec![french()]);

    // Rien ne bouge à la seconde restauration.
    let again = restore(&pool, &cipher, &lot, true).await.expect("seconde restauration");
    assert_eq!(counts(&again, "status_pages"), (0, 0, 1), "{again:#?}");

    // Un texte modifié ici est remis à celui du fichier.
    let mut changed = french();
    changed.title = "Autre".into();
    status_lang::replace(&pool, restored, "de", &[changed]).await.unwrap();
    let back = restore(&pool, &cipher, &lot, true).await.expect("troisième restauration");
    assert_eq!(counts(&back, "status_pages"), (0, 1, 0), "{back:#?}");
    assert_eq!(status_lang::list(&pool, restored).await.unwrap(), vec![french()]);
}

#[tokio::test]
async fn une_ancienne_sauvegarde_garde_les_langues_en_place() {
    let old: Bundle = serde_json::from_value(json!({
        "version": 1,
        "created_at": "2026-09-01 10:00:00",
        "source_version": "0.19.0",
        "status_pages": [
            { "slug": "vieille", "title": "Vieille", "published": true, "theme": "auto",
              "show_uptime_days": 30 }
        ]
    }))
    .expect("ancien lot lisible");
    assert_eq!(old.status_pages[0].default_lang, None);
    assert!(old.status_pages[0].translations.is_empty());

    let dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&dir).await;
    restore(&pool, &cipher, &old, true).await.expect("restauration");
    let id = page_id(&pool, "vieille").await;
    status_lang::replace(&pool, id, "es", &[french()]).await.unwrap();

    let report = restore(&pool, &cipher, &old, true).await.expect("seconde restauration");
    assert_eq!(counts(&report, "status_pages"), (0, 0, 1), "{report:#?}");
    assert_eq!(status_lang::default_lang(&pool, id).await.unwrap(), "es");
    assert_eq!(status_lang::list(&pool, id).await.unwrap().len(), 1);
}
