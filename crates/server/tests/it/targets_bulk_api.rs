//! Modification groupée (`PATCH /api/targets/bulk`), canaux propres à un
//! moniteur et modèles de moniteur (`/api/monitor-templates`).

use crate::common;

use axum::http::StatusCode;
use dumbmonit_server::db;
use serde_json::{Value, json};

use common::TestApp;

struct Lab {
    app: TestApp,
    pool: sqlx::SqlitePool,
    admin: String,
}

async fn lab() -> Lab {
    let dir = tempfile::tempdir().unwrap();
    let config = common::base_config(dir.path());
    let pool = db::open(&config.database_path()).await.unwrap();
    let app = common::build(dir, config, pool.clone()).await;
    app.create_admin().await;
    let admin = app.admin_cookie().await;
    Lab { app, pool, admin }
}

async fn create_target(lab: &Lab, address: &str, folder: &str, tags: Value) -> i64 {
    let reply = lab
        .app
        .post(
            "/api/targets",
            json!({ "name": address, "address": address, "kind": "dummy",
                    "group_name": folder, "tags": tags }),
            Some(&lab.admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["id"].as_i64().unwrap()
}

async fn create_channel(lab: &Lab, name: &str) -> i64 {
    let reply = lab
        .app
        .post(
            "/api/notify/channels",
            json!({ "name": name, "kind": "discord",
                    "secrets": { "webhook_url": "https://127.0.0.1:1/hook/abcdef" } }),
            Some(&lab.admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["id"].as_i64().unwrap()
}

async fn bulk(lab: &Lab, cookie: &str, body: Value) -> common::Reply {
    lab.app.request("PATCH", "/api/targets/bulk", Some(body), Some(cookie)).await
}

#[tokio::test]
async fn un_lot_change_periode_canaux_etiquettes_et_dossier() {
    let lab = lab().await;
    let a = create_target(&lab, "10.0.0.1", "", json!({ "env": "lab" })).await;
    let b = create_target(&lab, "10.0.0.2", "Old", json!({})).await;
    let ops = create_channel(&lab, "ops").await;
    let mail = create_channel(&lab, "mail").await;

    let reply = bulk(
        &lab,
        &lab.admin,
        json!({ "ids": [a, b, 999_999, a], "patch": {
            "interval_secs": 300,
            "channels": { "set": [ops, mail] },
            "tags": { "add": { "team": "infra" }, "remove": ["env"] },
            "group_name": "Edge"
        } }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["updated"], 2);
    assert_eq!(reply.body["failed"], 1);
    let results = reply.body["results"].as_array().unwrap();
    assert_eq!(results.len(), 3, "a repeated id counts once: {results:?}");
    assert_eq!(results[2]["id"], 999_999);
    assert_eq!(results[2]["ok"], false);
    assert!(results[2]["error"].as_str().unwrap().contains("not found"));

    for id in [a, b] {
        let target = lab.app.get(&format!("/api/targets/{id}"), Some(&lab.admin)).await.body;
        assert_eq!(target["interval_secs"], 300);
        assert_eq!(target["group_name"], "Edge");
        assert_eq!(target["tags"]["team"], "infra");
        assert!(target["tags"].get("env").is_none());
        let mut want = vec![ops, mail];
        want.sort_unstable();
        assert_eq!(target["channels"], json!(want));
    }

    // Retirer un canal, puis le lot apparaît au journal d'audit.
    let reply = bulk(
        &lab,
        &lab.admin,
        json!({ "ids": [a], "patch": { "channels": { "remove": [mail] } } }),
    )
    .await;
    assert_eq!(reply.body["updated"], 1, "{}", reply.body);
    assert_eq!(db::target_channels::of(&lab.pool, a).await.unwrap(), vec![ops]);
    let audited: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = 'targets.bulk_update'")
            .fetch_one(&lab.pool)
            .await
            .unwrap();
    assert_eq!(audited, 2);
}

#[tokio::test]
async fn un_lot_invalide_est_refuse_en_entier() {
    let lab = lab().await;
    let a = create_target(&lab, "10.0.0.1", "", json!({})).await;
    let nothing = bulk(&lab, &lab.admin, json!({ "ids": [a], "patch": {} })).await;
    assert_eq!(nothing.status, StatusCode::BAD_REQUEST);
    let too_fast =
        bulk(&lab, &lab.admin, json!({ "ids": [a], "patch": { "interval_secs": 5 } })).await;
    assert_eq!(too_fast.status, StatusCode::BAD_REQUEST);
    let unknown =
        bulk(&lab, &lab.admin, json!({ "ids": [a], "patch": { "channels": { "add": [4242] } } }))
            .await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST, "{}", unknown.body);
    let viewer = lab.app.viewer_cookie(&lab.admin).await;
    let refused =
        bulk(&lab, &viewer, json!({ "ids": [a], "patch": { "interval_secs": 120 } })).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
}

/// Un opérateur d'équipe ne modifie en lot que ce qu'il gère : le reste de la
/// sélection lui reste invisible.
#[tokio::test]
async fn un_operateur_d_equipe_ne_touche_que_ses_dossiers() {
    let lab = lab().await;
    let lyon = create_target(&lab, "10.2.0.1", "Lyon", json!({})).await;
    let paris = create_target(&lab, "10.1.0.1", "Paris", json!({})).await;
    let password = "operateur-mot-de-passe-solide";
    let reply = lab
        .app
        .post(
            "/api/users",
            json!({ "username": "op", "role": "operator", "password": password }),
            Some(&lab.admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let user = reply.body["id"].as_i64().unwrap();
    let reply = lab
        .app
        .post(
            "/api/teams",
            json!({ "name": "Lyon", "grants": [{ "folder": "Lyon", "level": "manage" }] }),
            Some(&lab.admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let team = reply.body["id"].as_i64().unwrap();
    let reply = lab
        .app
        .put(&format!("/api/teams/{team}"), json!({ "members": [user] }), Some(&lab.admin))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let operator = lab.app.login_as("op", password).await.cookie();

    let reply =
        bulk(&lab, &operator, json!({ "ids": [lyon, paris], "patch": { "interval_secs": 120 } }))
            .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["updated"], 1);
    assert_eq!(reply.body["results"][1]["ok"], false);
    let secs: i64 = sqlx::query_scalar("SELECT interval_secs FROM targets WHERE id = ?")
        .bind(paris)
        .fetch_one(&lab.pool)
        .await
        .unwrap();
    assert_eq!(secs, 60, "Paris is not theirs");

    let moved =
        bulk(&lab, &operator, json!({ "ids": [lyon], "patch": { "group_name": "Paris" } })).await;
    assert_eq!(moved.status, StatusCode::FORBIDDEN, "{}", moved.body);
}

#[tokio::test]
async fn un_moniteur_s_enregistre_comme_modele_sans_ses_secrets() {
    let lab = lab().await;
    let ops = create_channel(&lab, "ops").await;
    let reply = lab
        .app
        .post(
            "/api/targets",
            json!({ "name": "nas", "address": "10.0.0.9", "kind": "dummy", "interval_secs": 120,
                    "tags": { "env": "prod", "api_key": "s3cret" }, "channels": [ops] }),
            Some(&lab.admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    assert_eq!(reply.body["channels"], json!([ops]));
    let target = reply.body["id"].as_i64().unwrap();

    let save = json!({ "name": "NAS", "target_id": target });
    let reply = lab.app.post("/api/monitor-templates", save.clone(), Some(&lab.admin)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let template = reply.body.clone();
    assert_eq!(template["kind"], "dummy");
    assert_eq!(template["interval_secs"], 120);
    assert_eq!(template["tags"], json!({ "env": "prod" }), "never a secret");
    assert_eq!(template["channels"], json!([ops]));
    let id = template["id"].as_i64().unwrap();

    let duplicate = lab.app.post("/api/monitor-templates", save, Some(&lab.admin)).await;
    assert_eq!(duplicate.status, StatusCode::CONFLICT);

    let path = format!("/api/monitor-templates/{id}");
    let renamed = lab.app.put(&path, json!({ "name": "Storage" }), Some(&lab.admin)).await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(renamed.body["name"], "Storage");
    let list = lab.app.get("/api/monitor-templates", Some(&lab.admin)).await;
    assert_eq!(list.body.as_array().map(Vec::len), Some(1));

    let viewer = lab.app.viewer_cookie(&lab.admin).await;
    let refused = lab.app.delete(&path, Some(&viewer)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let deleted = lab.app.delete(&path, Some(&lab.admin)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let gone = lab.app.get(&path, Some(&lab.admin)).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}
