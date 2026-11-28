//! Repères de changement : création par un jeton (le cas d'un pipeline CI),
//! lecture par fenêtre et par moniteur, droits des équipes, alerte qui cite le
//! repère qui la précède.

use crate::common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::TestApp;
use dumbmonit_server::db;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

/// Requête portée par un jeton : ni cookie ni en-tête anti-CSRF, comme `curl`.
async fn with_token(
    app: &TestApp,
    method: &str,
    uri: &str,
    body: Option<Value>,
    token: &str,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let request = match body {
        Some(value) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.router.clone().oneshot(request).await.expect("réponse");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("corps").to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn token(app: &TestApp, admin: &str, scope: &str) -> String {
    let reply = app.post("/api/tokens", json!({ "name": "ci", "scope": scope }), Some(admin)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["secret"].as_str().expect("secret").to_string()
}

async fn target(app: &TestApp, admin: &str, name: &str, address: &str, folder: &str) -> i64 {
    let reply = app
        .post(
            "/api/targets",
            json!({ "name": name, "address": address, "kind": "dummy", "group_name": folder,
                    "tags": { "env": "prod" } }),
            Some(admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["id"].as_i64().unwrap()
}

async fn user(app: &TestApp, admin: &str, name: &str, role: &str) -> (i64, String) {
    let password = format!("{name}-mot-de-passe-solide");
    let reply = app
        .post(
            "/api/users",
            json!({ "username": name, "role": role, "password": password }),
            Some(admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    (reply.body["id"].as_i64().unwrap(), app.login_as(name, &password).await.cookie())
}

async fn make_as(app: &TestApp, cookie: &str, body: Value) -> Value {
    let reply = app.post("/api/annotations", body, Some(cookie)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body
}

async fn status_of_post(app: &TestApp, cookie: &str, body: Value) -> StatusCode {
    app.post("/api/annotations", body, Some(cookie)).await.status
}

async fn patch(app: &TestApp, cookie: &str, id: i64, body: Value) -> common::Reply {
    app.request("PATCH", &format!("/api/annotations/{id}"), Some(body), Some(cookie)).await
}

async fn remove(app: &TestApp, cookie: &str, id: i64) -> StatusCode {
    app.delete(&format!("/api/annotations/{id}"), Some(cookie)).await.status
}

/// Titres de la liste que `uri` rend, dans l'ordre.
async fn titles_at(app: &TestApp, cookie: &str, uri: &str) -> Vec<String> {
    let reply = app.get(uri, Some(cookie)).await;
    assert_eq!(reply.status, StatusCode::OK, "{uri}: {}", reply.body);
    let list = reply.body.as_array().expect("liste");
    list.iter().map(|item| item["title"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn a_write_token_documents_a_deployment_and_a_read_token_cannot() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let writer = token(&app, &admin, "write").await;
    let reader = token(&app, &admin, "read").await;

    let body = json!({ "title": "Deploy v2.4.1", "link": "https://ci.example/runs/42",
                       "source": "ci", "tags": ["deploy"] });
    let (status, created) =
        with_token(&app, "POST", "/api/annotations", Some(body.clone()), &writer).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["title"], "Deploy v2.4.1");
    assert_eq!(created["source"], "ci");
    assert_eq!(created["scope"], json!({ "kind": "global" }));
    assert_eq!(created["author"], "token:ci");
    assert!(created["until"].is_null());
    assert!(!created["at"].as_str().unwrap().ends_with('Z'), "UTC without suffix");

    let (status, refused) = with_token(&app, "POST", "/api/annotations", Some(body), &reader).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    let (status, listed) = with_token(&app, "GET", "/api/annotations", None, &reader).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1);

    // Sans jeton ni session : refusé.
    assert_eq!(app.get("/api/annotations", None).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn markers_are_listed_by_window_and_by_monitor() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let paris = target(&app, &admin, "paris", "10.0.0.1", "Paris/Rack").await;
    let lyon = target(&app, &admin, "lyon", "10.0.0.2", "Lyon").await;

    let paris_scope = json!({ "kind": "monitors", "targets": [paris] });
    let folder_scope = json!({ "kind": "folder", "folder": "Paris" });
    let tag_scope = json!({ "kind": "tags", "tags": ["env=prod"] });
    let markers = [
        json!({ "title": "global", "at": "2026-11-20T10:00:00Z" }),
        json!({ "title": "paris only", "at": "2026-11-20T11:00:00Z", "scope": paris_scope }),
        json!({ "title": "folder", "at": "2026-11-20T12:00:00Z", "scope": folder_scope }),
        json!({ "title": "tag", "at": "2026-11-20T13:00:00Z", "scope": tag_scope }),
        json!({ "title": "window", "at": "2026-11-21T08:00:00Z", "until": "2026-11-21T10:00:00Z" }),
        json!({ "title": "later", "at": "2026-11-25T08:00:00Z" }),
    ];
    for marker in markers {
        make_as(&app, &admin, marker).await;
    }

    assert_eq!(
        titles_at(&app, &admin, "/api/annotations").await,
        ["global", "paris only", "folder", "tag", "window", "later"]
    );

    // Une fenêtre qui en recoupe une autre la montre, même si elle a commencé avant.
    let uri = "/api/annotations?from=2026-11-21T09:00:00Z&to=2026-11-22T00:00:00Z";
    assert_eq!(titles_at(&app, &admin, uri).await, ["window"]);
    let uri = "/api/annotations?from=2026-11-20T10:30:00Z&to=2026-11-20T12:30:00Z";
    assert_eq!(titles_at(&app, &admin, uri).await, ["paris only", "folder"]);

    let until = "to=2026-11-22T00:00:00Z";
    let uri = format!("/api/annotations?target={paris}&{until}");
    assert_eq!(
        titles_at(&app, &admin, &uri).await,
        ["global", "paris only", "folder", "tag", "window"]
    );
    let uri = format!("/api/annotations?target={lyon}&{until}");
    assert_eq!(titles_at(&app, &admin, &uri).await, ["global", "tag", "window"]);

    let missing = app.get("/api/annotations?target=999", Some(&admin)).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    let bad = app.get("/api/annotations?from=yesterday", Some(&admin)).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn markers_are_validated_edited_and_deleted() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;

    for bad in [
        json!({ "title": "  " }),
        json!({ "title": "x", "link": "javascript:alert(1)" }),
        json!({ "title": "x", "at": "2026-11-20T10:00:00Z", "until": "2026-11-20T09:00:00Z" }),
        json!({ "title": "x", "scope": { "kind": "monitors", "targets": [4242] } }),
        json!({ "title": "x", "source": "robot" }),
        json!({ "title": "x", "until": "2999-01-01T00:00:00Z" }),
    ] {
        let status = status_of_post(&app, &admin, bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }

    let created = make_as(
        &app,
        &admin,
        json!({ "title": "Deploy", "at": "2026-11-20T10:00:00Z", "until": "2026-11-20T10:10:00Z" }),
    )
    .await;
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["source"], "ui", "a session defaults to the ui source");

    let patched = patch(&app, &admin, id, json!({ "title": "Deploy v3", "until": null })).await;
    assert_eq!(patched.status, StatusCode::OK, "{}", patched.body);
    assert_eq!(patched.body["title"], "Deploy v3");
    assert!(patched.body["until"].is_null(), "null removes the window");
    assert_eq!(patched.body["at"], "2026-11-20 10:00:00", "untouched fields stay");

    assert_eq!(remove(&app, &admin, id).await, StatusCode::NO_CONTENT);
    assert_eq!(remove(&app, &admin, id).await, StatusCode::NOT_FOUND);

    // Un lecteur lit, ne pose rien.
    let viewer = app.viewer_cookie(&admin).await;
    assert_eq!(app.get("/api/annotations", Some(&viewer)).await.status, StatusCode::OK);
    let status = status_of_post(&app, &viewer, json!({ "title": "x" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Un opérateur sans restriction pose et retire.
    let (_, operator) = user(&app, &admin, "olga", "operator").await;
    let made = make_as(&app, &operator, json!({ "title": "Maintenance" })).await;
    assert_eq!(remove(&app, &operator, made["id"].as_i64().unwrap()).await, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_team_restricted_operator_only_touches_its_own_monitors() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let paris = target(&app, &admin, "paris", "10.0.0.1", "Paris").await;
    let lyon = target(&app, &admin, "lyon", "10.0.0.2", "Lyon").await;
    let (olga, cookie) = user(&app, &admin, "olga", "operator").await;
    let team = json!({ "name": "Paris", "members": [olga],
                       "grants": [{ "folder": "Paris", "level": "operate" }] });
    let reply = app.post("/api/teams", team, Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);

    let global = make_as(&app, &admin, json!({ "title": "global" })).await;
    let scope_lyon = json!({ "kind": "monitors", "targets": [lyon] });
    let lyon_only = make_as(&app, &admin, json!({ "title": "lyon", "scope": scope_lyon })).await;
    let lyon_id = lyon_only["id"].as_i64().unwrap();

    let scope = json!({ "kind": "monitors", "targets": [paris] });
    make_as(&app, &cookie, json!({ "title": "mine", "scope": scope })).await;
    let scope = json!({ "kind": "folder", "folder": "Paris" });
    make_as(&app, &cookie, json!({ "title": "folder", "scope": scope })).await;

    // Ni global, ni un moniteur d'une autre équipe, ni une portée qui déborde.
    let denied = |body: Value| status_of_post(&app, &cookie, body);
    assert_eq!(denied(json!({ "title": "all" })).await, StatusCode::FORBIDDEN);
    let scope = json!({ "kind": "monitors", "targets": [lyon] });
    assert_eq!(denied(json!({ "title": "x", "scope": scope })).await, StatusCode::FORBIDDEN);
    let scope = json!({ "kind": "monitors", "targets": [paris, lyon] });
    assert_eq!(denied(json!({ "title": "x", "scope": scope })).await, StatusCode::FORBIDDEN);
    let scope = json!({ "kind": "tags", "tags": ["env"] });
    let status = denied(json!({ "title": "x", "scope": scope })).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the tag reaches Lyon too");

    // Lecture : le global et le sien, jamais celui de Lyon.
    assert_eq!(titles_at(&app, &cookie, "/api/annotations").await, ["global", "mine", "folder"]);
    let hidden = app.get(&format!("/api/annotations?target={lyon}"), Some(&cookie)).await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);

    // Celui de Lyon est introuvable, et on n'y touche pas.
    let reply = patch(&app, &cookie, lyon_id, json!({ "title": "x" })).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(remove(&app, &cookie, lyon_id).await, StatusCode::NOT_FOUND);
    // Le global n'est pas à lui non plus.
    assert_eq!(remove(&app, &cookie, global["id"].as_i64().unwrap()).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_alert_cites_the_marker_of_the_half_hour_before() {
    let dir = tempfile::tempdir().unwrap();
    let config = common::base_config(dir.path());
    let pool = db::open(&config.database_path()).await.unwrap();
    let app = common::build(dir, config, pool.clone()).await;
    app.create_admin().await;
    let admin = app.admin_cookie().await;
    let web = target(&app, &admin, "web", "10.0.0.1", "").await;
    let other = target(&app, &admin, "other", "10.0.0.2", "").await;

    let now = chrono::Utc::now();
    for (fingerprint, target) in [("fp-web", web), ("fp-other", other)] {
        sqlx::query(
            "INSERT INTO alert_state (fingerprint, rule_uid, target_id, series_key, labels, phase,
                                      severity, firing_since, last_eval_at)
             VALUES (?, 'host_down', ?, ?, ?, 'firing', 'critical', ?, ?)",
        )
        .bind(fingerprint)
        .bind(target)
        .bind(format!("{{target=\"{target}\"}}"))
        .bind(format!("{{\"target\":\"{target}\"}}"))
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
    }
    let ten_minutes_ago = (now - chrono::TimeDelta::minutes(10)).to_rfc3339();
    let scope = json!({ "kind": "monitors", "targets": [web] });
    make_as(&app, &admin, json!({ "title": "Deploy v9", "at": ten_minutes_ago, "scope": scope }))
        .await;
    // Un repère d'avant la demi-heure ne compte pas.
    let old = (now - chrono::TimeDelta::hours(3)).to_rfc3339();
    make_as(&app, &admin, json!({ "title": "Old", "at": old })).await;

    let alerts = app.get("/api/alerts", Some(&admin)).await;
    assert_eq!(alerts.status, StatusCode::OK, "{}", alerts.body);
    let list = alerts.body.as_array().unwrap();
    let find = |fp: &str| list.iter().find(|a| a["fingerprint"] == fp).expect("alert").clone();
    assert_eq!(find("fp-web")["after_change"]["title"], "Deploy v9");
    assert!(find("fp-other")["after_change"].is_null(), "scoped to another monitor");
}
