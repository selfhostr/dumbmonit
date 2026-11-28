//! Les modèles de moniteur et les canaux propres à chaque moniteur voyagent
//! avec la sauvegarde : restaurés par nom et par référence, et relire le même
//! lot ne change rien.

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

async fn seed(pool: &SqlitePool) {
    for sql in [
        "INSERT INTO notification_channels (name, kind)
         VALUES ('ops', 'webhook'), ('mail', 'email')",
        "INSERT INTO targets (name, address, kind, interval_secs, enabled, tags)
         VALUES ('banc', '10.0.0.2', 'dummy', 60, 1, '{}')",
        "INSERT INTO target_channels (target_id, channel_id)
         SELECT t.id, c.id FROM targets t, notification_channels c
         WHERE t.address = '10.0.0.2' AND c.name = 'ops'",
        "INSERT INTO monitor_templates (name, kind, interval_secs, tags)
         VALUES ('Web check', 'http', 300, '{\"keyword\":\"ok\",\"env\":\"prod\"}')",
        "INSERT INTO monitor_template_channels (template_id, channel_id)
         SELECT t.id, c.id FROM monitor_templates t, notification_channels c
         WHERE c.name IN ('ops', 'mail')",
    ] {
        sqlx::query(sql).execute(pool).await.expect(sql);
    }
}

#[tokio::test]
async fn les_modeles_et_les_canaux_des_moniteurs_sont_sauvegardes_et_restaures() {
    let source_dir = tempfile::tempdir().unwrap();
    let (source, cipher) = instance(&source_dir).await;
    seed(&source).await;

    let lot = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    assert_eq!(lot.monitor_templates.len(), 1);
    let template = &lot.monitor_templates[0];
    assert_eq!((template.name.as_str(), template.kind.as_str()), ("Web check", "http"));
    assert_eq!(template.interval_secs, 300);
    assert_eq!(template.channels, vec!["mail".to_string(), "ops".to_string()]);
    assert_eq!(lot.target_channels.len(), 1);
    assert_eq!(lot.target_channels[0].target, "dummy|10.0.0.2");
    assert_eq!(lot.target_channels[0].channels, vec!["ops".to_string()]);

    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = instance(&target_dir).await;
    let report = restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(counts(&report, "monitor_templates"), (1, 0, 0), "{report:#?}");
    assert_eq!(counts(&report, "target_channels"), (1, 0, 0), "{report:#?}");

    let restored = db::monitor_templates::list(&pool).await.unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].tags.get("keyword").map(String::as_str), Some("ok"));
    assert_eq!(restored[0].channels.len(), 2);
    let target: i64 = sqlx::query_scalar("SELECT id FROM targets WHERE address = '10.0.0.2'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let ops: i64 = sqlx::query_scalar("SELECT id FROM notification_channels WHERE name = 'ops'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(db::target_channels::of(&pool, target).await.unwrap(), vec![ops]);

    let again = restore(&pool, &cipher, &lot, true).await.expect("seconde restauration");
    assert_eq!(counts(&again, "monitor_templates"), (0, 0, 1), "rien ne bouge : {again:#?}");
    assert_eq!(counts(&again, "target_channels"), (0, 0, 1), "rien ne bouge : {again:#?}");

    // Un modèle déjà présent reprend la période du lot.
    sqlx::query("UPDATE monitor_templates SET interval_secs = 60").execute(&pool).await.unwrap();
    let fixed = restore(&pool, &cipher, &lot, true).await.expect("restauration sur l'existant");
    assert_eq!(counts(&fixed, "monitor_templates"), (0, 1, 0), "{fixed:#?}");
    assert_eq!(db::monitor_templates::list(&pool).await.unwrap()[0].interval_secs, 300);
}
