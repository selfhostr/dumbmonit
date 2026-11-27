//! Tests d'intégration des SLO : cycle de vie, validation, droits.
//!
//! VictoriaMetrics est injoignable ici : l'état d'un SLO s'affiche tout de
//! même, marqué « non mesuré », sans disponibilité ni budget.

use crate::common;

use axum::http::StatusCode;
use serde_json::json;

use common::TestApp;

async fn create_target(app: &TestApp, cookie: &str, name: &str, address: &str, group: &str) -> i64 {
    let reply = app
        .post(
            "/api/targets",
            json!({ "name": name, "address": address, "kind": "dummy", "group_name": group }),
            Some(cookie),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "target: {}", reply.body);
    reply.body["id"].as_i64().expect("target id")
}

#[tokio::test]
async fn un_slo_se_cree_se_lit_se_modifie_et_se_supprime() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let nas = create_target(&app, &admin, "NAS", "10.0.0.5", "Storage").await;
    create_target(&app, &admin, "Router", "10.0.0.1", "").await;

    let reply = app
        .post(
            "/api/slos",
            json!({
                "name": "Storage availability",
                "objective": 99.9,
                "window_days": 30,
                "scope": { "targets": [nas], "groups": [] }
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let id = reply.body["id"].as_i64().expect("SLO id");
    assert_eq!(reply.body["name"], "Storage availability");
    assert_eq!(reply.body["objective"], 99.9);
    assert_eq!(reply.body["window_days"], 30);
    assert_eq!(reply.body["enabled"], true, "enabled by default");
    assert_eq!(reply.body["members"], json!([nas]));
    assert_eq!(reply.body["status"]["monitors"], 1);
    assert_eq!(reply.body["status"]["measured"], false, "no VictoriaMetrics in tests");
    assert!(reply.body["status"]["attained"].is_null());
    assert!(reply.body["status"]["budget"].is_null());
    assert_eq!(reply.body["status"]["verdict"], "none");
    assert!(reply.body["burndown"].as_array().is_some_and(Vec::is_empty));

    let list = app.get("/api/slos", Some(&admin)).await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.body.as_array().unwrap().len(), 1);
    assert_eq!(list.body[0]["status"]["monitors"], 1);

    // Un dossier entier : le SLO suit les moniteurs qui y entrent ensuite.
    let updated = app
        .put(
            &format!("/api/slos/{id}"),
            json!({
                "name": "Storage availability",
                "description": "Everything in the storage folder.",
                "objective": 99.5,
                "window_days": 7,
                "scope": { "targets": [], "groups": ["Storage"] },
                "enabled": false
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(updated.body["window_days"], 7);
    assert_eq!(updated.body["enabled"], false);
    assert_eq!(updated.body["scope"]["groups"], json!(["Storage"]));
    let later = create_target(&app, &admin, "NAS 2", "10.0.0.6", "Storage").await;
    let one = app.get(&format!("/api/slos/{id}"), Some(&admin)).await;
    assert_eq!(one.status, StatusCode::OK);
    assert_eq!(one.body["members"], json!([nas, later]));

    let removed = app.delete(&format!("/api/slos/{id}"), Some(&admin)).await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    let gone = app.get(&format!("/api/slos/{id}"), Some(&admin)).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
    let again = app.delete(&format!("/api/slos/{id}"), Some(&admin)).await;
    assert_eq!(again.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn les_valeurs_invalides_et_les_doublons_sont_refuses() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;

    let long = "x".repeat(600);
    let cases = [
        json!({ "name": "A", "objective": 100.0, "window_days": 30 }),
        json!({ "name": "A", "objective": 0.0, "window_days": 30 }),
        json!({ "name": "A", "objective": 99.9, "window_days": 14 }),
        json!({ "name": "  ", "objective": 99.9, "window_days": 30 }),
        json!({ "name": "A", "objective": 99.9, "window_days": 30, "description": long }),
    ];
    for body in cases {
        let reply = app.post("/api/slos", body.clone(), Some(&admin)).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{body} -> {}", reply.body);
    }

    let first = json!({ "name": "API", "objective": 99.9, "window_days": 28 });
    let reply = app.post("/api/slos", first.clone(), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let twin = app.post("/api/slos", first, Some(&admin)).await;
    assert_eq!(twin.status, StatusCode::CONFLICT, "{}", twin.body);
}

#[tokio::test]
async fn un_lecteur_lit_mais_ne_modifie_pas() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;

    let body = json!({ "name": "API", "objective": 99.9, "window_days": 30 });
    let created = app.post("/api/slos", body, Some(&admin)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.body["id"].as_i64().unwrap();

    let list = app.get("/api/slos", Some(&viewer)).await;
    assert_eq!(list.status, StatusCode::OK);
    let one = app.get(&format!("/api/slos/{id}"), Some(&viewer)).await;
    assert_eq!(one.status, StatusCode::OK);

    let body = json!({ "name": "B", "objective": 99.0, "window_days": 7 });
    let create = app.post("/api/slos", body, Some(&viewer)).await;
    assert_eq!(create.status, StatusCode::FORBIDDEN);
    let update = app
        .put(
            &format!("/api/slos/{id}"),
            json!({ "name": "API", "objective": 99.0, "window_days": 7 }),
            Some(&viewer),
        )
        .await;
    assert_eq!(update.status, StatusCode::FORBIDDEN);
    let delete = app.delete(&format!("/api/slos/{id}"), Some(&viewer)).await;
    assert_eq!(delete.status, StatusCode::FORBIDDEN);

    let anonymous = app.get("/api/slos", None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}
