//! La météo du mur : réglage par un administrateur, lecture par tout compte
//! connecté, appels à Open-Meteo faits par le serveur seul (faux Open-Meteo
//! local), cache de dix minutes, silence quand le service ne répond pas, et
//! voyage avec la sauvegarde.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use common::{TestApp, UNREACHABLE_VICTORIA};
use dumbmonit_server::backup::{ExportOptions, collect, restore};
use dumbmonit_server::state::{AppState, Inner};
use dumbmonit_server::weather::{Endpoints, WeatherHub};
use dumbmonit_server::{api, collectors, db, tsdb};
use serde_json::{Value, json};

#[derive(Default)]
struct Fake {
    forecasts: AtomicUsize,
    searches: AtomicUsize,
    down: AtomicBool,
}

async fn forecast(
    State(fake): State<Arc<Fake>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, axum::Json<Value>) {
    fake.forecasts.fetch_add(1, Ordering::SeqCst);
    if fake.down.load(Ordering::SeqCst) {
        return (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({ "error": true })));
    }
    assert_eq!(query["current"], "temperature_2m,weather_code,is_day");
    (
        StatusCode::OK,
        axum::Json(json!({ "current": { "temperature_2m": 11.6, "weather_code": 61, "is_day": 0 } })),
    )
}

async fn search(
    State(fake): State<Arc<Fake>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> axum::Json<Value> {
    fake.searches.fetch_add(1, Ordering::SeqCst);
    if query["name"] == "Nowhere" {
        return axum::Json(json!({ "generationtime_ms": 0.4 }));
    }
    axum::Json(json!({ "results": [
        { "name": "Lyon", "latitude": 45.75, "longitude": 4.85, "admin1": "Auvergne-Rhône-Alpes", "country": "France" },
        { "name": "Broken", "latitude": 400.0, "longitude": 4.85 }
    ] }))
}

struct Harness {
    app: TestApp,
    pool: sqlx::SqlitePool,
    fake: Arc<Fake>,
    admin: String,
}

async fn harness() -> Harness {
    let fake = Arc::new(Fake::default());
    let router = Router::new()
        .route("/v1/forecast", get(forecast))
        .route("/v1/search", get(search))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    let config = common::base_config(dir.path());
    let pool = db::open(&config.database_path()).await.unwrap();
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.unwrap();
    let mut registry = collectors::Registry::new();
    registry.register(Arc::new(collectors::DummyCollector));
    let victoria = tsdb::Victoria::new(UNREACHABLE_VICTORIA.to_string()).unwrap();
    let sink = tsdb::spawn_writer(victoria.clone(), Duration::from_secs(60));
    let state = AppState::new(Inner { config, pool: pool.clone(), cipher, victoria, sink, collectors: registry });
    let hub = WeatherHub::new(Endpoints { forecast: base.clone(), geocoding: base });
    let app = TestApp {
        router: api::router_with_hubs(state, dumbmonit_server::music::MusicHub::official(), hub),
        _dir: dir,
    };
    app.create_admin().await;
    let admin = app.admin_cookie().await;
    Harness { app, pool, fake, admin }
}

fn lyon() -> Value {
    json!({ "name": "Lyon", "region": "Auvergne-Rhône-Alpes", "country": "France",
            "latitude": 45.75, "longitude": 4.85 })
}

#[tokio::test]
async fn sans_lieu_personne_n_est_appele() {
    let h = harness().await;
    let reply = h.app.get("/api/wall/weather", Some(&h.admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["place"], Value::Null);
    assert_eq!(reply.body["current"], Value::Null);
    assert_eq!(reply.body["unit"], "c");
    assert_eq!(h.fake.forecasts.load(Ordering::SeqCst), 0);
    // Il faut une session : le mur anonyme ne lit rien.
    assert_eq!(h.app.get("/api/wall/weather", None).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn un_admin_choisit_le_lieu_et_tout_compte_lit_la_meteo_en_cache() {
    let h = harness().await;
    let viewer = h.app.viewer_cookie(&h.admin).await;

    let refused = h.app.put("/api/wall/weather", json!({ "place": lyon() }), Some(&viewer)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    let bad = json!({ "place": { "name": "X", "latitude": 99.0, "longitude": 0.0 } });
    assert_eq!(
        h.app.put("/api/wall/weather", bad, Some(&h.admin)).await.status,
        StatusCode::BAD_REQUEST
    );

    let saved = h
        .app
        .put("/api/wall/weather", json!({ "place": lyon(), "unit": "c" }), Some(&h.admin))
        .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(saved.body["current"]["temperature"], 12);
    assert_eq!(saved.body["current"]["condition"], "rain");
    assert_eq!(saved.body["current"]["is_day"], false);

    let seen = h.app.get("/api/wall/weather", Some(&viewer)).await;
    assert_eq!(seen.body["place"]["name"], "Lyon");
    assert_eq!(seen.body["current"]["temperature"], 12);
    // Deux lectures, une seule requête à Open-Meteo.
    assert_eq!(h.fake.forecasts.load(Ordering::SeqCst), 1);

    let fahrenheit = h
        .app
        .put("/api/wall/weather", json!({ "place": lyon(), "unit": "f" }), Some(&h.admin))
        .await;
    assert_eq!(fahrenheit.body["current"]["temperature"], 53);
    assert_eq!(h.fake.forecasts.load(Ordering::SeqCst), 1, "la conversion ne rappelle personne");

    let off = h.app.put("/api/wall/weather", json!({ "place": null }), Some(&h.admin)).await;
    assert_eq!(off.body["current"], Value::Null);
}

#[tokio::test]
async fn un_service_muet_ne_laisse_aucune_trace_sur_le_mur() {
    let h = harness().await;
    h.fake.down.store(true, Ordering::SeqCst);
    let reply =
        h.app.put("/api/wall/weather", json!({ "place": lyon() }), Some(&h.admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["place"]["name"], "Lyon");
    assert_eq!(reply.body["current"], Value::Null);
    // Un échec n'est pas redemandé à chaque lecture du mur.
    h.app.get("/api/wall/weather", Some(&h.admin)).await;
    assert_eq!(h.fake.forecasts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn la_recherche_de_ville_est_reservee_aux_admins_et_ecarte_les_lieux_invalides() {
    let h = harness().await;
    let viewer = h.app.viewer_cookie(&h.admin).await;
    let uri = "/api/wall/weather/places?q=Lyon&lang=fr";
    assert_eq!(h.app.get(uri, Some(&viewer)).await.status, StatusCode::FORBIDDEN);
    let found = h.app.get(uri, Some(&h.admin)).await;
    assert_eq!(found.status, StatusCode::OK);
    assert_eq!(found.body.as_array().unwrap().len(), 1, "le lieu hors limites est écarté");
    assert_eq!(found.body[0]["country"], "France");
    let none = h.app.get("/api/wall/weather/places?q=Nowhere", Some(&h.admin)).await;
    assert_eq!(none.body, json!([]));
    let short = h.app.get("/api/wall/weather/places?q=L", Some(&h.admin)).await;
    assert_eq!(short.status, StatusCode::BAD_REQUEST);
    assert_eq!(h.fake.searches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn le_reglage_voyage_avec_la_sauvegarde() {
    let source = harness().await;
    // Rien de choisi : rien n'est exporté.
    let pool = source.pool.clone();
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.unwrap();
    assert!(collect(&pool, &cipher, ExportOptions::default()).await.unwrap().wall_weather.is_none());

    source
        .app
        .put("/api/wall/weather", json!({ "place": lyon(), "unit": "f" }), Some(&source.admin))
        .await;
    let lot = collect(&pool, &cipher, ExportOptions::default()).await.unwrap();
    assert!(lot.wall_weather.is_some());

    let dir = tempfile::tempdir().unwrap();
    let target = db::open(&dir.path().join("dumbmonit.db")).await.unwrap();
    let target_cipher =
        db::init_cipher(&target, "secret-de-test-suffisamment-long").await.unwrap();
    restore(&target, &target_cipher, &lot, true).await.unwrap();
    let restored = dumbmonit_server::weather::load_settings(&target).await.unwrap();
    assert_eq!(restored.place.unwrap().name, "Lyon");
    assert_eq!(restored.unit, dumbmonit_server::weather::Unit::F);
}
