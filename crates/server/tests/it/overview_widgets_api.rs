//! `GET /api/overview/widgets` : la forme de la réponse, la dégradation sans
//! VictoriaMetrics, et la portée d'un compte restreint par ses équipes.
//!
//! Le scénario : deux dossiers, `Paris` et `Lyon`, une équipe Paris en lecture
//! seule, et une alerte déclenchée par équipement. Un lecteur de l'équipe Paris
//! ne doit rien voir de Lyon, ni dans les moniteurs, ni dans les chiffres des
//! alertes, ni dans les évènements, et ses requêtes de séries sont filtrées.

use crate::common;

use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use dumbmonit_server::db;
use serde_json::{Value, json};

use common::TestApp;

const SECRET_NAME: &str = "lyon-secret-nas";

/// Faux VictoriaMetrics : séries vides, et chaque chaîne de requête gardée.
async fn fake_victoria() -> (String, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let handler = move |uri: axum::http::Uri| {
        let record = record.clone();
        async move {
            record.lock().unwrap().push(uri.query().unwrap_or_default().to_string());
            let kind = if uri.path().ends_with("query_range") { "matrix" } else { "vector" };
            axum::Json(json!({ "status": "success", "data": { "resultType": kind, "result": [] } }))
        }
    };
    let router = axum::Router::new().fallback(handler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{address}"), seen)
}

async fn create_user(app: &TestApp, admin: &str, username: &str, role: &str) -> (i64, String) {
    let password = format!("{username}-mot-de-passe-solide");
    let reply = app
        .post(
            "/api/users",
            json!({ "username": username, "role": role, "password": password }),
            Some(admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let id = reply.body["id"].as_i64().unwrap();
    (id, app.login_as(username, &password).await.cookie())
}

async fn create_target(app: &TestApp, admin: &str, name: &str, address: &str, folder: &str) -> i64 {
    let reply = app
        .post(
            "/api/targets",
            json!({ "name": name, "address": address, "kind": "dummy", "group_name": folder }),
            Some(admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["id"].as_i64().unwrap()
}

/// Une alerte déclenchée il y a une heure sur `target`.
async fn fire(pool: &sqlx::SqlitePool, fingerprint: &str, target: i64, acked: bool) {
    let fired = chrono::Utc::now() - chrono::TimeDelta::hours(1);
    let acked_at = acked.then(|| (fired + chrono::TimeDelta::minutes(5)).to_rfc3339());
    sqlx::query(
        "INSERT INTO alert_history (fingerprint, rule_uid, target_id, from_phase, to_phase,
                                    severity, at, acked_at)
         VALUES (?, 'host_down', ?, 'pending', 'firing', 'critical', ?, ?)",
    )
    .bind(fingerprint)
    .bind(target)
    .bind(fired.to_rfc3339())
    .bind(acked_at)
    .execute(pool)
    .await
    .unwrap();
}

fn monitor_ids(body: &Value) -> Vec<i64> {
    let mut ids: Vec<i64> = body["sites"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|site| site["monitors"].as_array().unwrap().iter())
        .map(|m| m["id"].as_i64().unwrap())
        .collect();
    ids.sort_unstable();
    ids
}

#[tokio::test]
async fn a_team_reader_only_sees_its_own_monitors_alerts_and_series() {
    let (victoria, seen) = fake_victoria().await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = common::base_config(dir.path());
    config.victoria_url = Some(victoria);
    let pool = db::open(&config.database_path()).await.unwrap();
    let app = common::build(dir, config, pool.clone()).await;
    app.create_admin().await;
    let admin = app.admin_cookie().await;

    let paris = create_target(&app, &admin, "paris-router", "10.1.0.1", "Paris").await;
    let lyon = create_target(&app, &admin, SECRET_NAME, "10.66.66.66", "Lyon").await;
    let reply = app
        .post(
            "/api/teams",
            json!({ "name": "Paris", "grants": [{ "folder": "Paris", "level": "read" }] }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let team = reply.body["id"].as_i64().unwrap();
    let (viewer, cookie) = create_user(&app, &admin, "anna", "viewer").await;
    let reply =
        app.put(&format!("/api/teams/{team}"), json!({ "members": [viewer] }), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    fire(&pool, "fp-paris", paris, true).await;
    fire(&pool, "fp-lyon", lyon, false).await;

    // L'administrateur voit tout le parc.
    let reply = app.get("/api/overview/widgets?range=24h", Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let mut both = vec![paris, lyon];
    both.sort_unstable();
    assert_eq!(monitor_ids(&reply.body), both);
    assert_eq!(reply.body["alert_stats"]["total"], 2, "{}", reply.body["alert_stats"]);
    assert_eq!(reply.body["sites"][0]["key"], "local");
    assert!(reply.body.to_string().contains(SECRET_NAME));

    // Le lecteur de l'équipe Paris : son moniteur, son alerte, rien de Lyon.
    seen.lock().unwrap().clear();
    let reply = app.get("/api/overview/widgets", Some(&cookie)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(monitor_ids(&reply.body), vec![paris]);
    let stats = &reply.body["alert_stats"];
    assert_eq!(stats["total"], 1, "{stats}");
    assert_eq!(stats["acked_within_15m_pct"], 100.0, "{stats}");
    assert_eq!(stats["mtta_s"], 300, "{stats}");
    assert_eq!(stats["noisiest"]["target_id"], paris, "{stats}");
    assert_eq!(stats["days"].as_array().unwrap().len(), 30);
    for event in reply.body["events"].as_array().unwrap() {
        assert_eq!(event["target_id"], paris, "{event}");
    }
    assert!(reply.body["events"].as_array().unwrap().iter().any(|e| e["kind"] == "ack"));
    let text = reply.body.to_string();
    assert!(!text.contains(SECRET_NAME), "Lyon leaks: {text}");
    assert!(!text.contains(&format!("\"target_id\":{lyon}")), "Lyon leaks: {text}");
    // Chaque requête de séries porte le filtre de ses moniteurs.
    let queries = seen.lock().unwrap().clone();
    assert!(!queries.is_empty(), "no series query at all");
    for query in &queries {
        assert!(query.contains("extra_filters"), "unfiltered series query: {query}");
    }
}

#[tokio::test]
async fn without_victoriametrics_the_sql_parts_are_still_served() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let nas = create_target(&app, &admin, "nas", "10.0.0.5", "").await;

    let reply = app.get("/api/overview/widgets?range=7d", Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let body = &reply.body;
    assert_eq!(monitor_ids(body), vec![nas]);
    let monitor = &body["sites"][0]["monitors"][0];
    assert_eq!(monitor["category"], "servers");
    assert_eq!(monitor["kind"], "Demo device");
    for family in ["cpu", "mem", "disk", "net"] {
        assert_eq!(body["top"][family], json!([]), "{family}");
    }
    assert_eq!(body["services"], json!([]));
    assert!(body["sites"][0]["wan_latency_ms"].is_null());
    let availability = &body["availability"];
    assert_eq!(availability["days"].as_array().unwrap().len(), 30);
    assert!(availability["avg_30d"].is_null());
    assert_eq!(availability["slo_target"], 99.9);
    assert_eq!(body["alerts_opened"]["days"].as_array().unwrap().len(), 14);
    assert_eq!(body["agents"]["total"], 0);
    // Le moniteur ajouté à l'instant figure dans le fil.
    assert!(
        body["events"].as_array().unwrap().iter().any(|e| e["text"] == "Monitor added: nas"),
        "{}",
        body["events"]
    );
}

#[tokio::test]
async fn the_range_is_checked_and_wall_displays_are_refused() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;

    let reply = app.get("/api/overview/widgets?range=2d", Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);

    let reply = app.get("/api/overview/widgets", None).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);

    let viewer = app.viewer_cookie(&admin).await;
    let reply = app.get("/api/overview/widgets?range=1h", Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);

    let (_, wall) = create_user(&app, &admin, "lobby", "wall").await;
    let reply = app.get("/api/overview/widgets", Some(&wall)).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.body);
}
