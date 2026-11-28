//! Registre communautaire de paquets, contre un faux registre local. Dans son
//! propre binaire de test : le garde des envois sortants est global au processus.

#[path = "it/common/mod.rs"]
mod common;

use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;
use common::TestApp;
use serde_json::json;
use sha2::{Digest, Sha256};

const PACK: &str = include_str!("../../../packs/shelly-plug/pack.yaml");

async fn fake_registry(license: &'static str, sha: String) -> String {
    let index = json!({ "packs": [{
        "id": "shelly-plug", "name": "Shelly plug", "version": "1.0.0", "license": license,
        "author": "someone", "description": "Plug", "sha256": sha, "url": "shelly.yaml"
    }]});
    let app = Router::new()
        .route("/index.json", get(move || async move { axum::Json(index) }))
        .route("/shelly.yaml", get(|| async { PACK }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}/index.json")
}

fn sha_of_pack() -> String {
    hex::encode(Sha256::digest(PACK.as_bytes()))
}

async fn configure(app: &TestApp, admin: &str, url: &str) {
    let reply = app.put("/api/packs/registry/settings", json!({ "url": url }), Some(admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
}

#[tokio::test]
async fn the_registry_is_off_until_an_address_is_set_and_admin_only() {
    let app = TestApp::configured().await;
    assert_eq!(app.get("/api/packs/registry", None).await.status, StatusCode::UNAUTHORIZED);
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;
    let off = app.get("/api/packs/registry", Some(&admin)).await;
    assert_eq!(off.body["configured"], false);
    assert_eq!(off.body["packs"], json!([]));
    let denied = app.get("/api/packs/registry", Some(&viewer)).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let bad =
        app.put("/api/packs/registry/settings", json!({ "url": "ftp://x" }), Some(&admin)).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_pack_is_browsed_then_installed_from_the_registry() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    configure(&app, &admin, &fake_registry("MIT", sha_of_pack()).await).await;

    let listing = app.get("/api/packs/registry", Some(&admin)).await;
    assert_eq!(listing.body["error"], json!(null), "{}", listing.body);
    assert_eq!(listing.body["packs"][0]["status"], "available");

    let done =
        app.post("/api/packs/registry/install", json!({ "id": "shelly-plug" }), Some(&admin)).await;
    assert_eq!(done.status, StatusCode::CREATED, "{}", done.body);

    let after = app.get("/api/packs/registry", Some(&admin)).await;
    assert_eq!(after.body["packs"][0]["status"], "installed");
    assert_eq!(after.body["packs"][0]["installed_version"], "1.0.0");
}

#[tokio::test]
async fn a_wrong_checksum_or_licence_is_refused() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let body = json!({ "id": "shelly-plug" });

    configure(&app, &admin, &fake_registry("MIT", "0".repeat(64)).await).await;
    let bad_sha = app.post("/api/packs/registry/install", body.clone(), Some(&admin)).await;
    assert_eq!(bad_sha.status, StatusCode::BAD_REQUEST);

    configure(&app, &admin, &fake_registry("GPL-3.0", sha_of_pack()).await).await;
    let bad_licence = app.post("/api/packs/registry/install", body, Some(&admin)).await;
    assert_eq!(bad_licence.status, StatusCode::BAD_REQUEST);
    let none =
        app.post("/api/packs/registry/install", json!({ "id": "other" }), Some(&admin)).await;
    assert_eq!(none.status, StatusCode::NOT_FOUND);
    assert_eq!(app.get("/api/packs/shelly-plug", Some(&admin)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_registry_address_travels_with_the_backup() {
    use dumbmonit_server::backup::{ExportOptions, collect, restore};
    use dumbmonit_server::db;
    use dumbmonit_server::packs::registry::{self, Settings};

    let secret = "secret-de-test-suffisamment-long";
    let dir = tempfile::tempdir().unwrap();
    let source = db::open(&dir.path().join("source.db")).await.unwrap();
    let cipher = db::init_cipher(&source, secret).await.unwrap();
    let empty = collect(&source, &cipher, ExportOptions::default()).await.unwrap();
    assert!(empty.pack_registry.is_none());

    let url = "https://example.github.io/packs/index.json".to_string();
    registry::save(&source, &Settings { url: url.clone() }).await.unwrap();
    let bundle = collect(&source, &cipher, ExportOptions::default()).await.unwrap();
    assert!(bundle.pack_registry.is_some());

    let target = db::open(&dir.path().join("target.db")).await.unwrap();
    let target_cipher = db::init_cipher(&target, secret).await.unwrap();
    restore(&target, &target_cipher, &bundle, true).await.unwrap();
    assert_eq!(registry::load(&target).await.unwrap().url, url);
}
