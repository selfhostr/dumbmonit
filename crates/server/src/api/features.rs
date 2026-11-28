//! Modules facultatifs ([`crate::features`]).
//!
//! * `GET /settings/features` — l'état des modules, pour tout compte connecté.
//! * `PUT /settings/features` — administrateur ; chaque champ absent garde sa
//!   valeur. Rend l'objet complet.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use crate::api::ApiResult;
use crate::auth::audit;
use crate::auth::client_ip::ClientIp;
use crate::auth::middleware::AdminIdentity;
use crate::features::{self, Features, FeaturesPatch};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/settings/features", get(read).put(update))
}

async fn read(State(state): State<AppState>) -> Json<Features> {
    Json(features::load(&state.pool).await)
}

async fn update(
    AdminIdentity(principal): AdminIdentity,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(patch): Json<FeaturesPatch>,
) -> ApiResult<Json<Features>> {
    let mut value = features::load(&state.pool).await;
    let changed = value.apply(patch);
    features::save(&state.pool, &value).await?;
    // Les noms des modules touchés seulement.
    if !changed.is_empty() {
        audit::record(
            &state.pool,
            Some(&principal.label()),
            "features.settings",
            Some(&changed.join(", ")),
            ip,
        )
        .await;
    }
    Ok(Json(value))
}
