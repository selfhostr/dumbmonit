//! Tests d'intégration des post-mortems d'incident : brouillon, publication
//! après résolution, lecture publique, droits.

use crate::common;

use axum::http::StatusCode;
use serde_json::{Value, json};

use common::TestApp;

/// Vérifie qu'aucune clé interdite n'apparaît nulle part dans le document.
fn assert_no_key(value: &Value, forbidden: &[&str]) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                assert!(!forbidden.contains(&key.as_str()), "forbidden key `{key}`");
                assert_no_key(child, forbidden);
            }
        }
        Value::Array(items) => items.iter().for_each(|child| assert_no_key(child, forbidden)),
        _ => {}
    }
}

fn draft() -> Value {
    json!({
        "summary": "The NAS lost a disk and its pool went read-only.",
        "impact": "Backups failed for 40 minutes.",
        "root_cause": "A disk past its rated life.",
        "timeline": [
            { "at": "2026-11-01T10:20:00Z", "text": "Disk replaced." },
            { "at": "2026-11-01T10:00:00Z", "text": "Pool read-only." }
        ],
        "actions": [{ "text": "Keep a spare disk on site", "done": false }]
    })
}

#[tokio::test]
async fn un_post_mortem_se_publie_une_fois_l_incident_resolu() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let page = app
        .post(
            "/api/status-pages",
            json!({ "title": "Lab", "slug": "pm-lab", "published": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(page.status, StatusCode::CREATED, "{}", page.body);
    let page_id = page.body["id"].as_i64().unwrap();

    let reply = app
        .post(
            "/api/incidents",
            json!({ "title": "NAS read-only", "kind": "incident", "page_id": page_id,
                    "body": "Looking into it." }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let id = reply.body["id"].as_i64().unwrap();
    assert!(reply.body["postmortem"].is_null(), "no post-mortem yet");
    let missing = app.get(&format!("/api/incidents/{id}/postmortem"), Some(&admin)).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    // Brouillon pendant l'incident : enregistré, trié, invisible du public.
    let saved = app.put(&format!("/api/incidents/{id}/postmortem"), draft(), Some(&admin)).await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    let postmortem = &saved.body["postmortem"];
    assert_eq!(postmortem["published"], false);
    assert!(postmortem["published_at"].is_null());
    assert_eq!(postmortem["timeline"][0]["text"], "Pool read-only.", "sorted by time");
    assert_eq!(postmortem["actions"][0]["done"], false);
    let public = app.get("/api/public/status/pm-lab", None).await;
    assert!(public.body["incidents"][0].get("postmortem").is_none(), "{}", public.body);

    // Publier avant la résolution est refusé.
    let mut published = draft();
    published["published"] = json!(true);
    let early =
        app.put(&format!("/api/incidents/{id}/postmortem"), published.clone(), Some(&admin)).await;
    assert_eq!(early.status, StatusCode::BAD_REQUEST, "{}", early.body);

    let resolved = app
        .post(
            &format!("/api/incidents/{id}/updates"),
            json!({ "status": "resolved", "body": "Disk replaced." }),
            Some(&admin),
        )
        .await;
    assert_eq!(resolved.status, StatusCode::CREATED, "{}", resolved.body);
    assert_eq!(resolved.body["postmortem"]["summary"], draft()["summary"], "kept on the view");

    let done = app.put(&format!("/api/incidents/{id}/postmortem"), published, Some(&admin)).await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    assert_eq!(done.body["postmortem"]["published"], true);
    let published_at = done.body["postmortem"]["published_at"].clone();
    assert!(published_at.is_string(), "{}", done.body);

    let public = app.get("/api/public/status/pm-lab", None).await;
    assert_eq!(public.status, StatusCode::OK);
    let shown = &public.body["incidents"][0]["postmortem"];
    assert_eq!(shown["root_cause"], "A disk past its rated life.");
    assert_eq!(shown["timeline"].as_array().unwrap().len(), 2);
    assert_eq!(shown["published_at"], published_at);
    assert_no_key(&public.body, &["id", "incident_id", "page_id"]);
    assert!(shown.get("updated_at").is_none() && shown.get("published").is_none());

    // Une retouche garde la date de première publication.
    let mut touched = draft();
    touched["published"] = json!(true);
    touched["actions"][0]["done"] = json!(true);
    let again = app.put(&format!("/api/incidents/{id}/postmortem"), touched, Some(&admin)).await;
    assert_eq!(again.status, StatusCode::OK, "{}", again.body);
    assert_eq!(again.body["postmortem"]["published_at"], published_at);
    assert_eq!(again.body["postmortem"]["actions"][0]["done"], true);

    // Retiré : la page publique n'en parle plus.
    let removed = app.delete(&format!("/api/incidents/{id}/postmortem"), Some(&admin)).await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    let public = app.get("/api/public/status/pm-lab", None).await;
    assert!(public.body["incidents"][0].get("postmortem").is_none());
    let twice = app.delete(&format!("/api/incidents/{id}/postmortem"), Some(&admin)).await;
    assert_eq!(twice.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn une_maintenance_n_a_pas_de_post_mortem_et_un_lecteur_n_ecrit_rien() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;

    let maintenance = app
        .post(
            "/api/incidents",
            json!({
                "title": "Firmware upgrade",
                "kind": "maintenance",
                "starts_at": "2026-01-01T10:00:00Z",
                "ends_at": "2099-01-01T12:00:00Z"
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(maintenance.status, StatusCode::CREATED, "{}", maintenance.body);
    let id = maintenance.body["id"].as_i64().unwrap();
    let refused = app.put(&format!("/api/incidents/{id}/postmortem"), draft(), Some(&admin)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{}", refused.body);

    let incident = app
        .post("/api/incidents", json!({ "title": "Outage", "kind": "incident" }), Some(&admin))
        .await;
    let id = incident.body["id"].as_i64().unwrap();
    let forbidden =
        app.put(&format!("/api/incidents/{id}/postmortem"), draft(), Some(&viewer)).await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    let unknown = app.put("/api/incidents/999999/postmortem", draft(), Some(&admin)).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}
