//! Annuaire des pages de statut (`GET /api/public/status`) : seules les pages
//! publiées, publiques et laissées dans l'annuaire y figurent.

use crate::common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use common::TestApp;

async fn create(app: &TestApp, admin: &str, body: Value) -> i64 {
    let reply = app.post("/api/status-pages", body, Some(admin)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["id"].as_i64().unwrap()
}

fn slugs(body: &Value) -> Vec<String> {
    body["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .map(|page| page["slug"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn the_directory_lists_only_published_public_listed_pages() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;

    let open =
        create(&app, &admin, json!({ "title": "Open", "slug": "open", "published": true })).await;
    create(&app, &admin, json!({ "title": "Draft", "slug": "draft", "published": false })).await;
    let locked =
        create(&app, &admin, json!({ "title": "Locked", "slug": "locked", "published": true }))
            .await;
    let members =
        create(&app, &admin, json!({ "title": "Members", "slug": "members", "published": true }))
            .await;
    create(
        &app,
        &admin,
        json!({ "title": "Hidden", "slug": "hidden", "published": true, "listed": false }),
    )
    .await;

    let reply = app
        .put(
            &format!("/api/status-pages/{locked}/access"),
            json!({ "mode": "password", "password": "locked-secret-1" }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let reply = app
        .put(
            &format!("/api/status-pages/{members}/access"),
            json!({ "mode": "users" }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);

    // Une page créée figure dans l'annuaire par défaut ; le réglage est servi.
    let view = app.get(&format!("/api/status-pages/{open}"), Some(&admin)).await;
    assert_eq!(view.body["listed"], true);

    // Sans session : c'est une route publique.
    let reply = app.get("/api/public/status", None).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(slugs(&reply.body), ["open"]);
    let page = &reply.body["pages"][0];
    assert_eq!(page["title"], "Open");
    assert_eq!(page["default_lang"], "en");
    assert_eq!(page["languages"], json!(["en"]));
    assert!(page["url"].as_str().unwrap().ends_with("/s/open"), "{page}");

    // Retirée de l'annuaire, la page reste lisible par son adresse.
    let reply = app
        .put(
            &format!("/api/status-pages/{open}"),
            json!({ "title": "Open", "slug": "open", "published": true, "listed": false }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["listed"], false);
    let reply = app.get("/api/public/status", None).await;
    assert!(slugs(&reply.body).is_empty(), "{}", reply.body);
    assert_eq!(app.get("/api/public/status/open", None).await.status, StatusCode::OK);

    // Une mise à jour qui ne dit rien du réglage le garde.
    let reply = app
        .put(
            &format!("/api/status-pages/{open}"),
            json!({ "title": "Open again", "slug": "open", "published": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.body["listed"], false, "{}", reply.body);

    languages(&app, &admin).await;
}

/// Suite du test ci-dessus, dans le même : la liste est gardée en mémoire pour
/// tout le processus, deux tests en parallèle se la partageraient.
async fn languages(app: &TestApp, admin: &str) {
    let id =
        create(app, admin, json!({ "title": "Home", "slug": "home", "published": true })).await;
    let reply = app
        .put(
            &format!("/api/status-pages/{id}/languages"),
            json!({ "default_lang": "fr",
                    "translations": [{ "lang": "de", "title": "Startseite" }] }),
            Some(admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);

    let reply = app.get("/api/public/status", None).await;
    assert_eq!(slugs(&reply.body), ["home"]);
    let page = &reply.body["pages"][0];
    assert_eq!(page["slug"], "home");
    // Le titre est celui écrit ; les langues, la langue par défaut d'abord.
    assert_eq!(page["title"], "Home");
    assert_eq!(page["default_lang"], "fr");
    assert_eq!(page["languages"], json!(["fr", "de"]));
}
