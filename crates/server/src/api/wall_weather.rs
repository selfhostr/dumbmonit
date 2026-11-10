//! La météo du mode mur (`crate::weather`).
//!
//! - `GET /api/wall/weather` : le réglage (lieu, unité) et, quand un lieu est
//!   choisi, la météo du moment. Lisible par tout porteur authentifié, dans les
//!   mêmes conditions que le reste des données du mur (`/api/music/now`,
//!   alertes…). Sans lieu, rien n'est demandé à personne ; si Open-Meteo ne
//!   répond pas, `current` vaut `null` et le mur n'affiche rien.
//! - `PUT /api/wall/weather` : choisit le lieu et l'unité (administrateurs).
//! - `GET /api/wall/weather/places?q=…` : recherche de ville par nom
//!   (administrateurs : le serveur n'est pas un relais de géocodage public).

use axum::extract::{Extension, Query, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::api::{ApiError, ApiResult};
use crate::auth::audit;
use crate::auth::client_ip::ClientIp;
use crate::auth::middleware::{AdminIdentity, Identity};
use crate::state::AppState;
use crate::weather::{self, Current, Place, Settings, Unit, WeatherHub};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/wall/weather", get(overview).put(save))
        .route("/wall/weather/places", get(places))
}

#[derive(Debug, Serialize)]
pub struct Overview {
    pub place: Option<Place>,
    pub unit: Unit,
    pub current: Option<Current>,
}

fn no_store(body: impl IntoResponse) -> Response {
    let mut response = body.into_response();
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn build(state: &AppState, hub: &WeatherHub) -> ApiResult<Overview> {
    let settings = weather::load_settings(&state.pool).await?;
    let current = match &settings.place {
        Some(place) => hub.current(place, settings.unit).await,
        None => None,
    };
    Ok(Overview { place: settings.place, unit: settings.unit, current })
}

/// `GET /api/wall/weather`.
async fn overview(
    State(state): State<AppState>,
    Extension(hub): Extension<WeatherHub>,
    Identity(_): Identity,
) -> ApiResult<Response> {
    Ok(no_store(Json(build(&state, &hub).await?)))
}

/// `PUT /api/wall/weather` : le réglage en entier ; `place: null` coupe la météo.
async fn save(
    State(state): State<AppState>,
    Extension(hub): Extension<WeatherHub>,
    AdminIdentity(me): AdminIdentity,
    ClientIp(ip): ClientIp,
    Json(settings): Json<Settings>,
) -> ApiResult<Response> {
    if let Some(place) = &settings.place {
        place.validate().map_err(|why| ApiError::BadRequest(why.into()))?;
    }
    let settings = Settings {
        place: settings.place.map(|p| Place { name: p.name.trim().to_string(), ..p }),
        ..settings
    };
    weather::save_settings(&state.pool, &settings).await?;
    audit::record(&state.pool, Some(&me.label()), "wall.weather", None, ip).await;
    Ok(no_store(Json(build(&state, &hub).await?)))
}

#[derive(Debug, Deserialize)]
struct PlacesQuery {
    q: String,
    #[serde(default)]
    lang: Option<String>,
}

/// `GET /api/wall/weather/places?q=Lyon&lang=fr` : jusqu'à six villes.
async fn places(
    Extension(hub): Extension<WeatherHub>,
    AdminIdentity(_): AdminIdentity,
    Query(query): Query<PlacesQuery>,
) -> ApiResult<Response> {
    let q = query.q.trim();
    if q.chars().count() < 2 || q.chars().count() > 80 || q.chars().any(char::is_control) {
        return Err(ApiError::BadRequest("Type at least two letters of a city name.".into()));
    }
    let lang = query
        .lang
        .as_deref()
        .filter(|l| (2..=5).contains(&l.len()) && l.chars().all(|c| c.is_ascii_alphabetic()))
        .unwrap_or("en");
    let found = hub.search(q, lang).await.ok_or_else(|| {
        ApiError::BadGateway("The weather service did not answer. Try again in a moment.".into())
    })?;
    Ok(no_store(Json(found)))
}
