//! Registre communautaire de paquets : parcourir l'index, installer ou mettre
//! à jour un paquet, régler l'adresse du registre. Administrateurs seulement.
//!
//! Rien ne part tant qu'aucune adresse n'est saisie. Le téléchargement passe
//! par `packs::registry` (garde des envois sortants, taille bornée) ; le paquet
//! n'est installé qu'après contrôle de l'empreinte, de la licence et de la
//! version minimale, par le même chemin que `POST /api/packs`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Extension, Json, Router};
use dumbmonit_pack::Pack;
use serde::{Deserialize, Serialize};

use crate::api::packs::{InstallReport, admin, install_parsed};
use crate::api::{ApiError, ApiResult};
use crate::auth::audit;
use crate::auth::client_ip::ClientIp;
use crate::auth::middleware::CurrentPrincipal;
use crate::packs;
use crate::packs::registry::{self, Entry, Settings};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/packs/registry", get(browse))
        .route("/packs/registry/settings", put(set_settings))
        .route("/packs/registry/install", post(install))
}

/// Un paquet du registre, avec son état par rapport à l'instance.
#[derive(Debug, Serialize)]
pub struct RegistryPack {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub min_version: Option<String>,
    /// Version installée, si le paquet l'est.
    pub installed_version: Option<String>,
    /// `available`, `installed`, `update` ou `incompatible` (version de
    /// DumbMonit trop ancienne, ou licence hors de la liste permise).
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RegistryView {
    pub url: String,
    pub configured: bool,
    pub packs: Vec<RegistryPack>,
    /// Lignes de l'index écartées, avec la raison.
    pub skipped: Vec<String>,
    /// Pourquoi l'index n'a pas pu être lu ; `null` si tout va bien.
    pub error: Option<String>,
}

fn status_of(entry: &Entry, installed: Option<&str>) -> &'static str {
    if !registry::license_allowed(&entry.license)
        || !registry::compatible(entry.min_version.as_deref(), env!("CARGO_PKG_VERSION"))
    {
        return "incompatible";
    }
    match installed {
        None => "available",
        Some(version) if registry::is_newer(&entry.version, version) => "update",
        Some(_) => "installed",
    }
}

async fn read_index(url: &str) -> Result<(Vec<Entry>, Vec<String>), String> {
    let index_url = registry::resolve(url, "")?;
    let bytes = registry::fetch(&index_url, registry::MAX_INDEX_BYTES).await?;
    registry::parse_index(&bytes)
}

pub async fn browse(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
) -> ApiResult<Json<RegistryView>> {
    admin(&principal)?;
    let url = registry::load(&state.pool).await?.url;
    let configured = !url.is_empty();
    let mut view = RegistryView { configured, url, packs: vec![], skipped: vec![], error: None };
    if !view.configured {
        return Ok(Json(view));
    }
    match read_index(&view.url).await {
        Ok((entries, skipped)) => {
            let installed = packs::list(&state.pool).await?;
            view.skipped = skipped;
            view.packs = entries
                .into_iter()
                .map(|entry| {
                    let current =
                        installed.iter().find(|p| p.id == entry.id).map(|p| p.version.clone());
                    RegistryPack {
                        status: status_of(&entry, current.as_deref()),
                        installed_version: current,
                        id: entry.id,
                        name: entry.name,
                        version: entry.version,
                        description: entry.description,
                        author: entry.author,
                        license: entry.license,
                        min_version: entry.min_version,
                    }
                })
                .collect();
        }
        Err(message) => view.error = Some(message),
    }
    Ok(Json(view))
}

#[derive(Debug, Deserialize)]
pub struct SettingsPayload {
    pub url: String,
}

pub async fn set_settings(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
    ClientIp(ip): ClientIp,
    Json(payload): Json<SettingsPayload>,
) -> ApiResult<Json<Settings>> {
    let actor = admin(&principal)?;
    let url = registry::validate_url(&payload.url).map_err(ApiError::BadRequest)?;
    let settings = Settings { url };
    registry::save(&state.pool, &settings).await?;
    audit::record(&state.pool, Some(&actor), "pack.registry_settings", None, ip).await;
    Ok(Json(settings))
}

#[derive(Debug, Deserialize)]
pub struct InstallPayload {
    pub id: String,
}

pub async fn install(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
    ClientIp(ip): ClientIp,
    Json(payload): Json<InstallPayload>,
) -> ApiResult<(StatusCode, Json<InstallReport>)> {
    let actor = admin(&principal)?;
    let url = registry::load(&state.pool).await?.url;
    if url.is_empty() {
        return Err(ApiError::BadRequest("No pack registry is configured.".into()));
    }
    let (entries, _) = read_index(&url).await.map_err(ApiError::BadGateway)?;
    let entry = entries
        .into_iter()
        .find(|entry| entry.id == payload.id)
        .ok_or_else(|| {
            ApiError::NotFound(format!("Pack \"{}\" is not in the registry.", payload.id))
        })?;
    if !registry::license_allowed(&entry.license) {
        return Err(ApiError::BadRequest(format!(
            "The licence \"{}\" is not accepted: packs must be MIT, Apache-2.0, BSD, ISC, \
             MPL-2.0, CC0 or Unlicense.",
            entry.license
        )));
    }
    if !registry::compatible(entry.min_version.as_deref(), env!("CARGO_PKG_VERSION")) {
        return Err(ApiError::BadRequest(format!(
            "This pack needs DumbMonit {} or later.",
            entry.min_version.as_deref().unwrap_or("?")
        )));
    }
    let pack_url = registry::resolve(&url, &entry.url).map_err(ApiError::BadRequest)?;
    let bytes =
        registry::fetch(&pack_url, registry::MAX_PACK_BYTES).await.map_err(ApiError::BadGateway)?;
    registry::verify_sha256(&bytes, &entry.sha256).map_err(ApiError::BadRequest)?;
    let yaml = String::from_utf8(bytes)
        .map_err(|_| ApiError::BadRequest("The pack must be UTF-8 text.".into()))?;
    let pack = Pack::parse(&yaml).map_err(|error| {
        ApiError::BadRequest(format!(
            "This pack cannot be installed:\n- {}",
            error.errors.join("\n- ")
        ))
    })?;
    if pack.id() != entry.id {
        return Err(ApiError::BadRequest("The pack's id differs from the registry's.".into()));
    }
    let (status, report) = install_parsed(&state, pack, &actor, ip).await?;
    audit::record(
        &state.pool,
        Some(&actor),
        "pack.registry_installed",
        Some(&format!("{} {}", entry.id, entry.version)),
        ip,
    )
    .await;
    Ok((status, Json(report)))
}
