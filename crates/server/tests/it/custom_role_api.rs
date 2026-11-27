//! Rôles personnalisés, de bout en bout : un compte reçoit un jeu de
//! permissions du catalogue, le garde d'API ne lui ouvre que leurs routes, et
//! l'administration des comptes, des rôles et des jetons reste à
//! l'administrateur.

use crate::common;

use axum::http::StatusCode;
use common::TestApp;
use dumbmonit_server::db;
use serde_json::{Value, json};

const EDITOR_PASSWORD: &str = "mot-de-passe-de-l-editeur";

struct Lab {
    app: TestApp,
    admin: String,
    editor: String,
    editor_id: i64,
    role: i64,
    target: i64,
}

/// Un administrateur, un rôle « Monitor editors » (`monitors.manage`), un
/// compte `ed` qui l'a, et un équipement.
async fn lab() -> Lab {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let config = common::base_config(dir.path());
    let pool = db::open(&config.database_path()).await.expect("ouverture de la base");
    db::alerts::seed_builtin_rules(&pool).await.expect("règles intégrées");
    let app = common::build(dir, config, pool).await;
    app.create_admin().await;
    let admin = app.admin_cookie().await;

    let role = app
        .post(
            "/api/roles",
            json!({ "name": "Monitor editors", "permissions": ["monitors.manage"] }),
            Some(&admin),
        )
        .await;
    assert_eq!(role.status, StatusCode::CREATED, "{}", role.body);
    assert_eq!(role.body["permissions"], json!(["view", "monitors.manage"]));
    let role_id = role.body["id"].as_i64().expect("id du rôle");

    let created = app
        .post(
            "/api/users",
            json!({
                "username": "ed",
                "role": "viewer",
                "custom_role_id": role_id,
                "password": EDITOR_PASSWORD
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["role"], json!("viewer"));
    assert_eq!(created.body["custom_role"]["name"], json!("Monitor editors"));
    let editor_id = created.body["id"].as_i64().expect("id");
    let editor = app.login_as("ed", EDITOR_PASSWORD).await.cookie();

    let target = app
        .post(
            "/api/targets",
            json!({ "name": "nas", "address": "10.0.0.7", "kind": "dummy" }),
            Some(&admin),
        )
        .await;
    assert_eq!(target.status, StatusCode::CREATED, "{}", target.body);
    let target = target.body["id"].as_i64().expect("id");

    Lab { app, admin, editor, editor_id, role: role_id, target }
}

fn silence() -> Value {
    json!({
        "name": "Fan swap",
        "matchers": { "host": "nas" },
        "schedule": {
            "kind": "once",
            "starts_at": "2026-01-01T00:00:00Z",
            "ends_at": "2099-01-01T02:00:00Z"
        }
    })
}

#[tokio::test]
async fn a_monitor_editor_edits_monitors_and_nothing_else() {
    let lab = lab().await;
    let app = &lab.app;
    let t = lab.target;

    let me = app.get("/api/auth/me", Some(&lab.editor)).await;
    assert_eq!(me.body["permissions"], json!(["view", "monitors.manage"]), "{}", me.body);

    let added = app
        .post(
            "/api/targets",
            json!({ "name": "router", "address": "10.0.0.1", "kind": "dummy" }),
            Some(&lab.editor),
        )
        .await;
    assert_eq!(added.status, StatusCode::CREATED, "{}", added.body);
    let renamed = app
        .put(
            &format!("/api/targets/{t}"),
            json!({ "name": "nas-2", "address": "10.0.0.7", "kind": "dummy" }),
            Some(&lab.editor),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);

    let refused: Vec<(&str, String, Option<Value>)> = vec![
        // Le traitement des alertes n'est pas dans son rôle.
        ("POST", format!("/api/alerts/cpu_high@{t}/ack"), Some(json!({}))),
        ("POST", "/api/alerts/silences".into(), Some(silence())),
        ("POST", "/api/alerts/history/dismiss-resolved".into(), Some(json!({}))),
        // Ni les notifications, ni les pages de statut.
        ("POST", "/api/notify/channels".into(), Some(json!({ "name": "c", "kind": "ntfy" }))),
        ("POST", "/api/status-pages".into(), Some(json!({ "title": "S", "slug": "s" }))),
        // Ni rien de l'administration.
        (
            "POST",
            "/api/users".into(),
            Some(json!({ "username": "eve", "role": "admin", "password": "mot-de-passe-pirate" })),
        ),
        ("PUT", format!("/api/users/{}", lab.editor_id), Some(json!({ "role": "admin" }))),
        ("POST", "/api/roles".into(), Some(json!({ "name": "All", "permissions": [] }))),
        (
            "PUT",
            format!("/api/roles/{}", lab.role),
            Some(json!({ "name": "Monitor editors", "permissions": ["alerts.operate"] })),
        ),
        ("POST", "/api/tokens".into(), Some(json!({ "name": "t", "scope": "write" }))),
        ("POST", "/api/backup".into(), Some(json!({}))),
        ("PUT", "/api/onboarding".into(), Some(json!({ "dismissed": true }))),
        ("PUT", "/api/notify/mail-server".into(), Some(json!({ "host": "smtp.example.org" }))),
        ("POST", format!("/api/targets/{t}/push/regenerate"), None),
    ];
    for (method, uri, body) in refused {
        let reply = app.request(method, &uri, body, Some(&lab.editor)).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {uri}: {}", reply.body);
    }
    for uri in ["/api/users", "/api/roles", "/api/roles/permissions", "/api/reports/schedules"] {
        let reply = app.get(uri, Some(&lab.editor)).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "GET {uri}");
    }
    // Il lit ce qu'un lecteur lit.
    for uri in ["/api/targets", "/api/alerts", "/api/alerts/rules", "/api/notify/channels"] {
        let reply = app.get(uri, Some(&lab.editor)).await;
        assert_eq!(reply.status, StatusCode::OK, "GET {uri}: {}", reply.body);
    }
    // Il est toujours ce qu'il était.
    let me = app.get("/api/auth/me", Some(&lab.editor)).await;
    assert_eq!(me.body["role"], json!("viewer"));
}

#[tokio::test]
async fn a_role_change_applies_at_once_and_a_role_in_use_stays() {
    let lab = lab().await;
    let app = &lab.app;

    // Le rôle gagne les maintenances : la session ouverte en profite tout de suite.
    let before = app.post("/api/alerts/silences", silence(), Some(&lab.editor)).await;
    assert_eq!(before.status, StatusCode::FORBIDDEN);
    let changed = app
        .put(
            &format!("/api/roles/{}", lab.role),
            json!({
                "name": "Monitor editors",
                "permissions": ["monitors.manage", "maintenance.manage", "reports.view"]
            }),
            Some(&lab.admin),
        )
        .await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    let after = app.post("/api/alerts/silences", silence(), Some(&lab.editor)).await;
    assert_eq!(after.status, StatusCode::CREATED, "{}", after.body);
    let reports = app.get("/api/reports/schedules", Some(&lab.editor)).await;
    assert_eq!(reports.status, StatusCode::OK, "{}", reports.body);
    // Toujours pas d'acquittement : il n'a pas `alerts.operate`.
    let ack = app
        .post(&format!("/api/alerts/cpu_high@{}/ack", lab.target), json!({}), Some(&lab.editor))
        .await;
    assert_eq!(ack.status, StatusCode::FORBIDDEN);

    // Un rôle attribué ne se supprime pas.
    let refused = app.delete(&format!("/api/roles/{}", lab.role), Some(&lab.admin)).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    let listed = app.get("/api/roles", Some(&lab.admin)).await;
    assert_eq!(listed.body[0]["users"], json!(1), "{}", listed.body);

    // Un rôle livré retire le rôle personnalisé ; le rôle se supprime ensuite.
    let user = format!("/api/users/{}", lab.editor_id);
    let back = app.put(&user, json!({ "role": "viewer" }), Some(&lab.admin)).await;
    assert_eq!(back.status, StatusCode::OK, "{}", back.body);
    assert!(back.body["custom_role"].is_null(), "{}", back.body);
    assert_eq!(back.body["permissions"], json!(["view"]));
    let silenced = app.post("/api/alerts/silences", silence(), Some(&lab.editor)).await;
    assert_eq!(silenced.status, StatusCode::FORBIDDEN);
    let deleted = app.delete(&format!("/api/roles/{}", lab.role), Some(&lab.admin)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    // Le journal d'audit en garde la trace.
    let audit = app.get("/api/auth/audit?limit=50", Some(&lab.admin)).await;
    let text = audit.body.to_string();
    for action in ["role.created", "role.updated", "role.deleted"] {
        assert!(text.contains(action), "{action} absent : {text}");
    }
}

#[tokio::test]
async fn roles_are_checked_on_the_way_in() {
    let lab = lab().await;
    let app = &lab.app;
    let attempts = [
        (json!({ "name": "", "permissions": [] }), StatusCode::BAD_REQUEST),
        (json!({ "name": "admin", "permissions": [] }), StatusCode::BAD_REQUEST),
        (json!({ "name": "Root", "permissions": ["users.manage"] }), StatusCode::BAD_REQUEST),
        (json!({ "name": "monitor EDITORS", "permissions": [] }), StatusCode::CONFLICT),
    ];
    for (body, status) in attempts {
        let reply = app.post("/api/roles", body.clone(), Some(&lab.admin)).await;
        assert_eq!(reply.status, status, "{body}: {}", reply.body);
    }
    // Un rôle personnalisé ne s'ajoute pas à un rôle d'administrateur.
    let reply = app
        .put(
            &format!("/api/users/{}", lab.editor_id),
            json!({ "role": "admin", "custom_role_id": lab.role }),
            Some(&lab.admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
    let reply = app
        .put(
            &format!("/api/users/{}", lab.editor_id),
            json!({ "custom_role_id": 9999 }),
            Some(&lab.admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
    // Le dernier administrateur ne passe pas à un rôle personnalisé.
    let reply =
        app.put("/api/users/1", json!({ "custom_role_id": lab.role }), Some(&lab.admin)).await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.body);

    let catalog = app.get("/api/roles/permissions", Some(&lab.admin)).await;
    assert_eq!(catalog.status, StatusCode::OK);
    let ids: Vec<&str> =
        catalog.body.as_array().unwrap().iter().filter_map(|p| p["id"].as_str()).collect();
    assert_eq!(ids[0], "view");
    assert!(ids.contains(&"monitors.manage") && ids.contains(&"alerts.operate"));
    assert!(!ids.iter().any(|id| id.starts_with("users") || id.starts_with("backup")));
}
