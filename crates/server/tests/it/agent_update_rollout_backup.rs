//! Le déploiement par vagues des mises à jour d'agents voyage avec la
//! sauvegarde : vagues, délai, seuil d'arrêt et pause. Une instance restaurée
//! ne relance pas toute sa flotte d'un coup, et un lot d'avant cette section
//! n'écrase rien.

use dumbmonit_server::backup::{Bundle, ExportOptions, RestoreReport, collect, restore};
use dumbmonit_server::collectors::agent::rollout::{self, Rollout, SETTINGS_KEY, Wave};
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

async fn stored(pool: &SqlitePool) -> bool {
    sqlx::query("SELECT 1 FROM settings WHERE key = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(pool)
        .await
        .expect("lecture")
        .is_some()
}

fn wanted() -> Rollout {
    Rollout {
        waves: vec![
            Wave::Count { value: 2 },
            Wave::Percent { value: 25 },
            Wave::Site { value: "Paris".into() },
            Wave::Tag { key: "env".into(), value: Some("prod".into()) },
        ],
        delay_minutes: 30,
        max_failure_percent: 10,
        paused: true,
        forgiven_before_ms: 0,
    }
}

#[tokio::test]
async fn la_politique_de_vagues_est_sauvegardee_et_restauree() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, cipher) = instance(&source_dir).await;
    rollout::save(&source, &wanted()).await.unwrap();

    let lot = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    assert!(lot.agent_update_rollout.is_some(), "la section est toujours écrite");

    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&target_dir).await;
    let dry = restore(&pool, &cipher, &lot, false).await.expect("simulation");
    assert_eq!(counts(&dry, "agent_update_rollout"), (1, 0, 0));
    assert!(!stored(&pool).await, "une simulation n'écrit rien");

    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "agent_update_rollout"), (1, 0, 0));
    assert_eq!(rollout::load(&pool).await.unwrap(), wanted());

    let again = restore(&pool, &cipher, &lot, true).await.expect("seconde restauration");
    assert_eq!(counts(&again, "agent_update_rollout"), (0, 0, 1), "rien ne change");
}

#[tokio::test]
async fn une_installation_neuve_exporte_le_comportement_historique() {
    let dir = tempfile::tempdir().unwrap();
    let (source, cipher) = instance(&dir).await;
    let lot = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    let section = lot.agent_update_rollout.clone().expect("section");
    let exported: Rollout = serde_json::from_value(section).unwrap();
    assert_eq!(exported, Rollout::default());
    assert!(exported.waves.is_empty(), "aucune vague : rien ne change pour un homelab");

    // Restaurer ce défaut sur une instance qui n'a rien réglé n'écrit rien.
    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&target_dir).await;
    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "agent_update_rollout"), (0, 0, 1));
    assert!(!stored(&pool).await);
}

#[tokio::test]
async fn un_lot_d_avant_la_section_laisse_la_politique_de_l_instance() {
    let lot: Bundle =
        serde_json::from_str(r#"{ "version": 1, "created_at": "2026-10-01T00:00:00Z" }"#).unwrap();
    assert!(lot.agent_update_rollout.is_none());

    let dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&dir).await;
    rollout::save(&pool, &wanted()).await.unwrap();
    restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(rollout::load(&pool).await.unwrap(), wanted());
}

#[tokio::test]
async fn un_reglage_illisible_ou_invalide_est_signale_et_ecarte() {
    for raw in [
        serde_json::json!({ "waves": [{ "by": "moon", "value": 3 }] }),
        serde_json::json!({ "waves": [{ "by": "percent", "value": 500 }] }),
        serde_json::json!({ "max_failure_percent": 101 }),
    ] {
        let lot: Bundle = serde_json::from_value(serde_json::json!({
            "version": 1,
            "created_at": "2026-10-01T00:00:00Z",
            "agent_update_rollout": raw,
        }))
        .expect("lot");
        let dir = tempfile::tempdir().unwrap();
        let (pool, cipher) = instance(&dir).await;
        let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
        assert_eq!(counts(&report, "agent_update_rollout"), (0, 0, 1));
        assert!(!stored(&pool).await);
    }
}
