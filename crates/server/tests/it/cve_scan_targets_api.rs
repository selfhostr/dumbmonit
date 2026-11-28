//! Contrôle CVE par équipement : `POST /api/security/cves/targets` active ou
//! coupe le contrôle de plusieurs équipements d'un coup. Un équipement écarté
//! disparaît de la liste et du résumé, et revient à la réactivation.

use crate::common;

use axum::http::StatusCode;
use common::TestApp;
use dumbmonit_server::db;
use serde_json::{Value, json};
use sqlx::SqlitePool;

async fn lab() -> (TestApp, SqlitePool) {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let config = common::base_config(dir.path());
    let pool = db::open(&config.database_path()).await.expect("base");
    let app = common::build(dir, config, pool.clone()).await;
    app.create_admin().await;
    (app, pool)
}

async fn create_target(app: &TestApp, admin: &str, name: &str, address: &str) -> i64 {
    let body = json!({ "name": name, "address": address, "kind": "dummy" });
    let reply = app.post("/api/targets", body, Some(admin)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "création : {}", reply.body);
    reply.body["id"].as_i64().expect("id")
}

async fn add_finding(pool: &SqlitePool, target: i64, cve: &str) {
    sqlx::query(
        "INSERT INTO cve_components (target_id, component, version, origin)
         VALUES (?, 'openssh', '9.0', 'agent')",
    )
    .bind(target)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO cve_findings (target_id, component, cve_id, installed_version, state,
                                   reason, source, first_seen)
         VALUES (?, 'openssh', ?, '9.0', 'vulnerable', 'below_fixed_version', 'nvd',
                 '2026-11-01T00:00:00Z')",
    )
    .bind(target)
    .bind(cve)
    .execute(pool)
    .await
    .unwrap();
}

fn listed_targets(body: &Value) -> Vec<i64> {
    let mut ids: Vec<i64> = body["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .map(|f| f["target_id"].as_i64().unwrap())
        .collect();
    ids.sort_unstable();
    ids
}

#[tokio::test]
async fn un_equipement_ecarte_disparait_puis_revient() {
    let (app, pool) = lab().await;
    let admin = app.admin_cookie().await;
    let a = create_target(&app, &admin, "alpha", "10.0.0.1").await;
    let b = create_target(&app, &admin, "bravo", "10.0.0.2").await;
    add_finding(&pool, a, "CVE-2024-6387").await;
    add_finding(&pool, b, "CVE-2024-6387").await;

    // Par défaut : tout est contrôlé, le champ est exposé.
    let one = app.get(&format!("/api/targets/{a}"), Some(&admin)).await;
    assert_eq!(one.body["cve_scan"], json!(true));
    let list = app.get("/api/security/cves", Some(&admin)).await;
    assert_eq!(listed_targets(&list.body), vec![a, b]);

    // Coupé pour `a` seulement (l'identifiant inconnu et le doublon sont sans effet).
    let reply = app
        .post(
            "/api/security/cves/targets",
            json!({ "target_ids": [a, a, 9999], "enabled": false }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["updated"], json!(1));

    let list = app.get("/api/security/cves", Some(&admin)).await;
    assert_eq!(listed_targets(&list.body), vec![b]);
    let summary = app.get("/api/security/cves/summary", Some(&admin)).await;
    assert_eq!(summary.body["counts"]["vulnerable"], json!(1));
    assert_eq!(summary.body["targets"], json!(1));
    let inventory = app.get("/api/security/cves/inventory", Some(&admin)).await;
    assert_eq!(inventory.body["components"].as_array().unwrap().len(), 1);
    let one = app.get(&format!("/api/targets/{a}"), Some(&admin)).await;
    assert_eq!(one.body["cve_scan"], json!(false));
    let all = app.get("/api/targets", Some(&admin)).await;
    let flags: Vec<(i64, bool)> = all
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["id"].as_i64().unwrap(), t["cve_scan"].as_bool().unwrap()))
        .collect();
    assert!(flags.contains(&(a, false)) && flags.contains(&(b, true)), "{flags:?}");

    // Répéter le même réglage ne change rien.
    let again = app
        .post(
            "/api/security/cves/targets",
            json!({ "target_ids": [a], "enabled": false }),
            Some(&admin),
        )
        .await;
    assert_eq!(again.body["updated"], json!(0));

    // Réactivé : les résultats (gardés en base) reviennent.
    let back = app
        .post(
            "/api/security/cves/targets",
            json!({ "target_ids": [a, b], "enabled": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(back.body["updated"], json!(1));
    let list = app.get("/api/security/cves", Some(&admin)).await;
    assert_eq!(listed_targets(&list.body), vec![a, b]);
}

#[tokio::test]
async fn le_formulaire_d_un_equipement_regle_aussi_le_controle() {
    let (app, _pool) = lab().await;
    let admin = app.admin_cookie().await;
    let created = app
        .post(
            "/api/targets",
            json!({ "name": "lab", "address": "10.0.0.9", "kind": "dummy", "cve_scan": false }),
            Some(&admin),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["cve_scan"], json!(false));
    let id = created.body["id"].as_i64().unwrap();

    // Une modification qui ne parle pas du champ le laisse tel quel.
    let renamed = app
        .put(
            &format!("/api/targets/{id}"),
            json!({ "name": "lab2", "address": "10.0.0.9", "kind": "dummy" }),
            Some(&admin),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(renamed.body["cve_scan"], json!(false));

    let enabled = app
        .put(
            &format!("/api/targets/{id}"),
            json!({ "name": "lab2", "address": "10.0.0.9", "kind": "dummy", "cve_scan": true }),
            Some(&admin),
        )
        .await;
    assert_eq!(enabled.body["cve_scan"], json!(true));
}

#[tokio::test]
async fn la_route_en_masse_est_validee_et_reservee_a_l_administrateur() {
    let (app, _pool) = lab().await;
    let admin = app.admin_cookie().await;
    let id = create_target(&app, &admin, "alpha", "10.0.0.1").await;

    let none = json!({ "target_ids": [], "enabled": false });
    let empty = app.post("/api/security/cves/targets", none, Some(&admin)).await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);

    let too_many: Vec<i64> = (1..=501).collect();
    let big = app
        .post(
            "/api/security/cves/targets",
            json!({ "target_ids": too_many, "enabled": false }),
            Some(&admin),
        )
        .await;
    assert_eq!(big.status, StatusCode::BAD_REQUEST);

    let viewer = app.viewer_cookie(&admin).await;
    let denied = app
        .post(
            "/api/security/cves/targets",
            json!({ "target_ids": [id], "enabled": false }),
            Some(&viewer),
        )
        .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);

    let one = app.get(&format!("/api/targets/{id}"), Some(&admin)).await;
    assert_eq!(one.body["cve_scan"], json!(true));
}
