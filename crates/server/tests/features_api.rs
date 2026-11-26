//! Modules facultatifs : lecture ouverte aux comptes connectés, écriture
//! réservée aux administrateurs, API coupée en 404, sauvegarde et migration.

mod common;

use axum::http::StatusCode;
use dumbmonit_server::backup::{ExportOptions, collect, restore};
use dumbmonit_server::db;
use dumbmonit_server::features::{self, Features};
use serde_json::json;

use common::TestApp;

#[tokio::test]
async fn un_module_coupe_repond_404_et_rallume_son_api() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;

    // Les tests partent avec tout actif : on coupe tout d'abord.
    let off = json!({ "wall": false, "sla_reports": false, "status_pages": false,
        "slo": false, "threat_map": false });
    let reply = app.put("/api/settings/features", off.clone(), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body, off);

    for uri in ["/api/slos", "/api/status-pages", "/api/reports/schedules", "/api/security/threats"]
    {
        let reply = app.get(uri, Some(&admin)).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}");
    }
    let public = app.get("/api/public/status/anything", None).await;
    assert_eq!(public.status, StatusCode::NOT_FOUND);

    // Un lecteur lit mais ne modifie pas.
    let read = app.get("/api/settings/features", Some(&viewer)).await;
    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(read.body, off);
    let denied = app.put("/api/settings/features", json!({ "slo": true }), Some(&viewer)).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);

    // Une modification partielle n'en touche qu'un.
    let reply = app.put("/api/settings/features", json!({ "slo": true }), Some(&admin)).await;
    assert_eq!(reply.body["slo"], true);
    assert_eq!(reply.body["status_pages"], false);
    assert_eq!(app.get("/api/slos", Some(&admin)).await.status, StatusCode::OK);
    assert_eq!(app.get("/api/status-pages", Some(&admin)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn les_modules_voyagent_avec_la_sauvegarde() {
    let source_dir = tempfile::tempdir().unwrap();
    let source = db::open(&source_dir.path().join("dumbmonit.db")).await.unwrap();
    let cipher = db::init_cipher(&source, "secret-de-test-suffisamment-long").await.unwrap();
    let chosen = Features { wall: true, status_pages: true, ..Features::default() };
    features::save(&source, &chosen).await.unwrap();

    let lot = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    assert_eq!(lot.features, Some(chosen));

    let target_dir = tempfile::tempdir().unwrap();
    let pool = db::open(&target_dir.path().join("dumbmonit.db")).await.unwrap();
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.unwrap();
    assert_eq!(features::load(&pool).await, Features::default());
    restore(&pool, &cipher, &lot, true).await.expect("restauration");
    assert_eq!(features::load(&pool).await, chosen);
}

#[tokio::test]
async fn la_migration_active_ce_qu_une_instance_utilise_deja() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(db::DATABASE_FILE);
    let old = db::open_up_to(&path, 308).await.expect("base ancienne");
    sqlx::query("INSERT INTO status_pages (slug, title) VALUES ('main', 'Main')")
        .execute(&old)
        .await
        .expect("page de statut");
    old.close().await;

    let pool = db::open(&path).await.expect("migration");
    let value = features::load(&pool).await;
    assert!(value.status_pages);
    assert!(!value.slo && !value.wall && !value.sla_reports && !value.threat_map);
}
