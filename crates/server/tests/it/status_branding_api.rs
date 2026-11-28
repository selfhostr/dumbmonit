//! Marque blanche d'une page de statut (« Hide DumbMonit branding ») et aperçu
//! de l'éditeur (`GET /api/status-pages/{id}/preview`).

use crate::common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use common::TestApp;

async fn create_page(app: &TestApp, cookie: &str, body: Value) -> Value {
    let reply = app.post("/api/status-pages", body, Some(cookie)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "page: {}", reply.body);
    reply.body
}

#[tokio::test]
async fn hide_branding_is_saved_kept_when_omitted_and_published() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;

    // Par défaut, la marque reste.
    let plain = create_page(&app, &admin, json!({ "title": "Plain", "published": true })).await;
    assert_eq!(plain["hide_branding"], false);
    let doc = app.get("/api/public/status/plain", None).await;
    assert_eq!(doc.status, StatusCode::OK, "{}", doc.body);
    assert_eq!(doc.body["page"]["hide_branding"], false);

    let page = create_page(
        &app,
        &admin,
        json!({ "title": "Acme", "published": true, "hide_branding": true }),
    )
    .await;
    assert_eq!(page["hide_branding"], true);
    let id = page["id"].as_i64().unwrap();

    // Le document public le dit, pour que l'interface retire l'icône du produit.
    let doc = app.get("/api/public/status/acme", None).await;
    assert_eq!(doc.status, StatusCode::OK, "{}", doc.body);
    assert_eq!(doc.body["page"]["hide_branding"], true);

    // Une mise à jour qui n'en parle pas garde le réglage…
    let reply = app
        .put(
            &format!("/api/status-pages/{id}"),
            json!({ "title": "Acme Cloud", "slug": "acme", "published": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["hide_branding"], true);

    // … et `false` le retire.
    let reply = app
        .put(
            &format!("/api/status-pages/{id}"),
            json!({
                "title": "Acme Cloud", "slug": "acme", "published": true, "hide_branding": false
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["hide_branding"], false);
    let reply = app.get(&format!("/api/status-pages/{id}"), Some(&admin)).await;
    assert_eq!(reply.body["hide_branding"], false);
}

#[tokio::test]
async fn the_editor_preview_shows_a_draft_with_its_monitors() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let reply = app
        .post(
            "/api/targets",
            json!({ "name": "NAS", "address": "10.0.0.5", "kind": "dummy" }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let nas = reply.body["id"].as_i64().unwrap();

    // Un brouillon : le public n'en sait rien.
    let page = create_page(&app, &admin, json!({ "title": "Draft", "slug": "draft" })).await;
    let id = page["id"].as_i64().unwrap();
    let reply = app
        .put(
            &format!("/api/status-pages/{id}/items"),
            json!([{ "target_id": nas, "label": "Storage", "group_name": "Back office" }]),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(app.get("/api/public/status/draft", None).await.status, StatusCode::NOT_FOUND);

    // L'aperçu, lui, le montre, avec la surveillance de chaque service.
    let reply = app.get(&format!("/api/status-pages/{id}/preview"), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["page"]["title"], "Draft");
    let item = &reply.body["groups"][0]["items"][0];
    assert_eq!(reply.body["groups"][0]["name"], "Back office");
    assert_eq!(item["label"], "Storage");
    assert_eq!(item["target_id"], nas);

    // Publiée, la page publique ne porte jamais l'identifiant interne.
    let reply = app
        .put(
            &format!("/api/status-pages/{id}"),
            json!({ "title": "Draft", "slug": "draft", "published": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let doc = app.get("/api/public/status/draft", None).await;
    assert_eq!(doc.status, StatusCode::OK, "{}", doc.body);
    assert!(doc.body["groups"][0]["items"][0].get("target_id").is_none(), "{}", doc.body);

    // L'aperçu est une route d'administration.
    let reply = app.get(&format!("/api/status-pages/{id}/preview"), None).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    let reply = app.get("/api/status-pages/9999/preview", Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}
