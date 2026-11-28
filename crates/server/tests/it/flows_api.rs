//! Flux réseau (bêta) : le réglage du récepteur est réservé aux
//! administrateurs, validé, journalisé, et voyage avec la sauvegarde. Les
//! tests ne lancent jamais le récepteur : aucun port UDP n'est ouvert.

use crate::common::TestApp;

use axum::http::StatusCode;
use dumbmonit_server::backup::{Bundle, ExportOptions, RestoreReport, collect, restore};
use dumbmonit_server::db;
use dumbmonit_server::flows::{self, Settings};
use serde_json::json;

#[tokio::test]
async fn le_reglage_est_reserve_aux_admins_valide_et_journalise() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;

    let first = app.get("/api/flows/settings", Some(&admin)).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_eq!(first.body["settings"]["enabled"], false);
    assert_eq!(first.body["settings"]["netflow_port"], 2055);
    assert_eq!(first.body["settings"]["sflow_port"], 6343);
    assert_eq!(first.body["listeners"].as_array().unwrap().len(), 2);
    assert!(first.body["counters"]["dropped_unknown"].is_u64());

    assert_eq!(app.get("/api/flows/settings", Some(&viewer)).await.status, StatusCode::FORBIDDEN);
    let on = json!({ "enabled": true, "netflow_port": 9995, "sflow_port": 6343 });
    assert_eq!(
        app.put("/api/flows/settings", on.clone(), Some(&viewer)).await.status,
        StatusCode::FORBIDDEN
    );
    let clash = json!({ "enabled": true, "netflow_port": 6343, "sflow_port": 6343 });
    assert_eq!(
        app.put("/api/flows/settings", clash, Some(&admin)).await.status,
        StatusCode::BAD_REQUEST
    );

    let saved = app.put("/api/flows/settings", on, Some(&admin)).await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(saved.body["settings"]["netflow_port"], 9995);
    let again = app.get("/api/flows/settings", Some(&admin)).await;
    assert_eq!(again.body["settings"]["enabled"], true);

    let audit = app.get("/api/auth/audit", Some(&admin)).await;
    let entries = audit.body.as_array().unwrap();
    let entry = entries.iter().find(|e| e["action"] == "flows.settings").expect("journalisé");
    assert_eq!(entry["subject"], "on, NetFlow/IPFIX 9995, sFlow 6343");
}

#[tokio::test]
async fn les_flux_d_un_moniteur_absent_ou_d_une_fenetre_inconnue_sont_refuses() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let missing = app.get("/api/targets/9999/flows?range=1h", Some(&admin)).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(app.get("/api/targets/1/flows", None).await.status, StatusCode::UNAUTHORIZED);
}

fn counts(report: &RestoreReport) -> (usize, usize, usize) {
    let section = report.sections.iter().find(|s| s.section == "flows").expect("section");
    (section.created, section.updated, section.skipped)
}

#[tokio::test]
async fn le_reglage_voyage_avec_la_sauvegarde() {
    let open = |dir: &tempfile::TempDir| {
        let path = dir.path().join("dumbmonit.db");
        async move {
            let pool = db::open(&path).await.expect("base");
            let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.unwrap();
            (pool, cipher)
        }
    };
    let source_dir = tempfile::tempdir().unwrap();
    let (source, source_cipher) = open(&source_dir).await;
    let lot = collect(&source, &source_cipher, ExportOptions::default()).await.unwrap();
    assert!(lot.flows.is_none(), "jamais réglé : rien dans le lot");

    let wanted = Settings { enabled: true, netflow_port: 2056, sflow_port: 6343 };
    flows::save(&source, &wanted).await.unwrap();
    let lot = collect(&source, &source_cipher, ExportOptions::default()).await.unwrap();
    assert!(lot.flows.is_some());

    let target_dir = tempfile::tempdir().unwrap();
    let (pool, cipher) = open(&target_dir).await;
    let dry = restore(&pool, &cipher, &lot, false).await.unwrap();
    assert_eq!(counts(&dry), (1, 0, 0));
    assert_eq!(flows::load(&pool).await.unwrap(), Settings::default(), "simulation sans écriture");
    let report = restore(&pool, &cipher, &lot, true).await.unwrap();
    assert_eq!(counts(&report), (1, 0, 0));
    assert_eq!(flows::load(&pool).await.unwrap(), wanted);
    let again = restore(&pool, &cipher, &lot, true).await.unwrap();
    assert_eq!(counts(&again), (0, 0, 1));

    // Un réglage illisible est signalé, celui de l'instance reste.
    let bad: Bundle = serde_json::from_value(json!({
        "version": 1,
        "created_at": "2026-11-01T00:00:00Z",
        "flows": { "enabled": true, "netflow_port": 6343, "sflow_port": 6343 },
    }))
    .unwrap();
    let report = restore(&pool, &cipher, &bad, true).await.unwrap();
    assert_eq!(counts(&report), (0, 0, 1));
    assert_eq!(flows::load(&pool).await.unwrap(), wanted);
}
