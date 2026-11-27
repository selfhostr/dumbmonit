//! Guide de premier lancement : l'état est dit par le serveur, l'achèvement est
//! verrouillé, les étapes écartées sont dédoublonnées dans l'ordre des étapes.

use crate::common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

#[tokio::test]
async fn une_instance_neuve_montre_le_guide_puis_se_souvient_de_ce_qui_est_ecarte() {
    let app = TestApp::configured().await;
    let cookie = app.admin_cookie().await;

    let reply = app.get("/api/onboarding", Some(&cookie)).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["complete"], false);
    assert_eq!(reply.body["has_target"], false);
    assert_eq!(reply.body["has_channel"], false);
    assert_eq!(reply.body["notification_confirmed"], false);
    assert!(reply.body["completed_at"].is_null());

    // « Skip » est retenu côté serveur, pas dans le navigateur.
    let reply = app.put("/api/onboarding", json!({ "skipped": true }), Some(&cookie)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["skipped"], true);

    // Les étapes écartées sont rangées dans l'ordre des étapes, sans doublon.
    let reply = app
        .put("/api/onboarding", json!({ "dismissed": ["test", "device", "test"] }), Some(&cookie))
        .await;
    assert_eq!(reply.body["dismissed"], json!(["device", "test"]));
    // Un champ absent garde sa valeur.
    assert_eq!(reply.body["skipped"], true);

    let reply = app
        .put("/api/onboarding", json!({ "dismissed": ["device", "inconnue"] }), Some(&cookie))
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert!(reply.body["error"].as_str().unwrap().contains("Unknown step"), "{}", reply.body);

    // Toutes les étapes écartées : la lecture suivante verrouille le guide.
    app.put(
        "/api/onboarding",
        json!({ "dismissed": ["device", "channel", "test"] }),
        Some(&cookie),
    )
    .await;
    let reply = app.get("/api/onboarding", Some(&cookie)).await;
    assert_eq!(reply.body["complete"], true);
    let locked_at = reply.body["completed_at"].clone();
    assert!(locked_at.is_string());

    // Le verrou ne bouge plus, même si on rouvre une étape.
    app.put("/api/onboarding", json!({ "dismissed": [] }), Some(&cookie)).await;
    let reply = app.get("/api/onboarding", Some(&cookie)).await;
    assert_eq!(reply.body["completed_at"], locked_at);
}

#[tokio::test]
async fn un_parc_deja_en_place_ne_se_voit_pas_proposer_le_tutoriel() {
    let app = TestApp::configured().await;
    let cookie = app.admin_cookie().await;

    let reply = app
        .post(
            "/api/targets",
            json!({ "name": "nas", "address": "nas.lan", "kind": "dummy" }),
            Some(&cookie),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let reply = app
        .post(
            "/api/notify/channels",
            json!({
                "name": "Ops", "kind": "discord",
                "secrets": { "webhook_url": "https://127.0.0.1:1/hook/x" }
            }),
            Some(&cookie),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);

    // Équipement et canal présents dès la première lecture : guide inutile.
    let reply = app.get("/api/onboarding", Some(&cookie)).await;
    assert_eq!(reply.body["has_target"], true);
    assert_eq!(reply.body["has_channel"], true);
    assert_eq!(reply.body["complete"], true, "{}", reply.body);
    assert_eq!(reply.body["notification_confirmed"], false);
}

#[tokio::test]
async fn les_etapes_faites_et_ecartees_se_completent_mutuellement() {
    let app = TestApp::configured().await;
    let cookie = app.admin_cookie().await;

    // Première lecture sur une instance vide : le guide démarre.
    let reply = app.get("/api/onboarding", Some(&cookie)).await;
    assert_eq!(reply.body["complete"], false);

    app.post(
        "/api/targets",
        json!({ "name": "nas", "address": "nas.lan", "kind": "dummy" }),
        Some(&cookie),
    )
    .await;
    app.post(
        "/api/notify/channels",
        json!({
            "name": "Ops", "kind": "discord",
            "secrets": { "webhook_url": "https://127.0.0.1:1/hook/x" }
        }),
        Some(&cookie),
    )
    .await;
    // Équipement et canal faits, test écarté : réglé, donc verrouillé.
    app.put("/api/onboarding", json!({ "dismissed": ["test"] }), Some(&cookie)).await;
    let reply = app.get("/api/onboarding", Some(&cookie)).await;
    assert_eq!(reply.body["complete"], true, "{}", reply.body);
}

#[tokio::test]
async fn seul_un_administrateur_ecrit_le_guide() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;

    let reply = app.get("/api/onboarding", Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::OK);
    let reply = app.put("/api/onboarding", json!({ "skipped": true }), Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.body);
}

#[tokio::test]
async fn le_premier_lancement_est_chronometre_sur_l_instance() {
    let app = TestApp::configured().await;
    let cookie = app.admin_cookie().await;

    // La création du compte ouvre la mesure ; le reste n'est pas encore franchi.
    let reply = app.get("/api/onboarding", Some(&cookie)).await;
    let timings = &reply.body["first_run"];
    assert!(timings["setup_at"].is_string(), "{}", reply.body);
    assert!(timings["first_target_at"].is_null());
    assert!(timings["first_channel_at"].is_null());
    assert!(timings["first_notification_at"].is_null());
    assert!(timings["first_alert_at"].is_null());

    let reply = app
        .post(
            "/api/targets",
            json!({ "name": "nas", "address": "nas.lan", "kind": "dummy" }),
            Some(&cookie),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let reply = app
        .post(
            "/api/notify/channels",
            json!({
                "name": "Ops", "kind": "discord",
                "secrets": { "webhook_url": "https://127.0.0.1:1/hook/x" }
            }),
            Some(&cookie),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let channel = reply.body["id"].as_i64().expect("identifiant du canal");

    let reply = app.get("/api/onboarding", Some(&cookie)).await;
    let first_target = reply.body["first_run"]["first_target_at"].clone();
    assert!(first_target.is_string(), "{}", reply.body);
    assert!(reply.body["first_run"]["first_channel_at"].is_string());
    // Un canal créé n'est pas un message parti.
    assert!(reply.body["first_run"]["first_notification_at"].is_null());

    // Un envoi réussi, consigné directement : le test n'a pas de réseau.
    let config = common::base_config(app._dir.path());
    let pool = dumbmonit_server::db::open(&config.database_path()).await.expect("base");
    let report = dumbmonit_server::notify::DeliveryReport {
        channel_id: channel,
        channel_name: "Ops".into(),
        error: None,
        transient: false,
    };
    dumbmonit_server::db::alerts::record_delivery(&pool, &report, chrono::Utc::now())
        .await
        .expect("envoi consigné");

    // Un second moniteur ne déplace pas le premier repère.
    app.post(
        "/api/targets",
        json!({ "name": "nas2", "address": "nas2.lan", "kind": "dummy" }),
        Some(&cookie),
    )
    .await;
    let reply = app.get("/api/onboarding", Some(&cookie)).await;
    assert!(reply.body["first_run"]["first_notification_at"].is_string(), "{}", reply.body);
    assert_eq!(reply.body["first_run"]["first_target_at"], first_target);
    // Seule une alerte du moteur compte comme première alerte.
    assert!(reply.body["first_run"]["first_alert_at"].is_null());
}
