//! Import de modèles fournis par l'administrateur : export de template Zabbix
//! (YAML ou XML), `generator.yml` ou `snmp.yml` de snmp_exporter.
//!
//! Le fichier est converti en paquet d'intégration (`dumbmonit_pack::import`).
//! `preview` ne touche à rien ; `apply` reconvertit le même fichier ici — le
//! YAML renvoyé par le client n'est jamais repris tel quel — puis l'installe
//! exactement comme `POST /api/packs` (table `packs`, donc sauvegardé avec
//! eux). Rien n'est embarqué dans DumbMonit : seul l'utilisateur apporte le
//! fichier, sous sa responsabilité.
//!
//! Corps JSON : `{"content": "…", "filename"?, "id"?, "label"?, "sysobjectid"?}`.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Extension, Json, Router};
use dumbmonit_pack::Pack;
use dumbmonit_pack::import::{Conversion, ImportOptions, MAX_INPUT_BYTES, SourceFormat, convert};
use serde::{Deserialize, Serialize};

use crate::api::packs::{InstallReport, admin, install_parsed};
use crate::api::{ApiError, ApiResult};
use crate::auth::audit;
use crate::auth::client_ip::ClientIp;
use crate::auth::middleware::CurrentPrincipal;
use crate::packs;
use crate::state::AppState;

/// Corps accepté : le fichier (2 Mio au plus) une fois échappé en JSON.
pub const MAX_BODY_BYTES: usize = MAX_INPUT_BYTES * 2;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/import/templates/preview",
            post(preview).layer(DefaultBodyLimit::max(MAX_BODY_BYTES)),
        )
        .route("/import/templates/apply", post(apply).layer(DefaultBodyLimit::max(MAX_BODY_BYTES)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportBody {
    content: String,
    /// Nom du fichier choisi, pour le journal d'audit seulement.
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    sysobjectid: Vec<String>,
}

fn read(body: &Bytes) -> ApiResult<(ImportBody, Conversion)> {
    let body: ImportBody = crate::untrusted::from_slice(body)
        .map_err(|error| ApiError::BadRequest(format!("Invalid request: {error}")))?;
    if body.content.len() > MAX_INPUT_BYTES {
        return Err(ApiError::BadRequest(format!(
            "The file is larger than {} KiB.",
            MAX_INPUT_BYTES / 1024
        )));
    }
    let options = ImportOptions {
        id: body.id.clone(),
        label: body.label.clone(),
        sysobjectid: body.sysobjectid.clone(),
    };
    let conversion =
        convert(&body.content, &options).map_err(|error| ApiError::BadRequest(error.0))?;
    Ok((body, conversion))
}

fn format_name(format: SourceFormat) -> &'static str {
    match format {
        SourceFormat::ZabbixYaml => "Zabbix YAML",
        SourceFormat::ZabbixXml => "Zabbix XML",
        SourceFormat::SnmpExporterGenerator => "snmp_exporter generator.yml",
        SourceFormat::SnmpExporterSnmp => "snmp_exporter snmp.yml",
    }
}

// --------------------------------------------------------------------------
// POST /api/import/templates/preview
// --------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Preview {
    #[serde(flatten)]
    pub conversion: Conversion,
    /// Faux tant que `errors` n'est pas vide.
    pub installable: bool,
    /// Un paquet de même identifiant est déjà installé : l'appliquer le met à jour.
    pub exists: bool,
}

pub async fn preview(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
    body: Bytes,
) -> ApiResult<Json<Preview>> {
    admin(&principal)?;
    let (_, conversion) = read(&body)?;
    let exists = packs::get(&state.pool, &conversion.pack_id).await?.is_some();
    Ok(Json(Preview { installable: conversion.is_installable(), exists, conversion }))
}

// --------------------------------------------------------------------------
// POST /api/import/templates/apply
// --------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Applied {
    pub pack_id: String,
    /// Type de cible créé (`pack.<id>`), `null` pour des profils SNMP seuls.
    pub kind: Option<String>,
    pub snmp_profiles: Vec<String>,
    /// `uid` des règles d'alerte du paquet.
    pub rules: Vec<String>,
    pub install: InstallReport,
}

pub async fn apply(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
    ClientIp(ip): ClientIp,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Applied>)> {
    let actor = admin(&principal)?;
    let (body, conversion) = read(&body)?;
    if !conversion.is_installable() {
        return Err(ApiError::BadRequest(format!(
            "This template cannot be installed:\n- {}",
            conversion.errors.join("\n- ")
        )));
    }
    let pack = Pack::parse(&conversion.yaml).map_err(|error| {
        ApiError::BadRequest(format!(
            "This template cannot be installed:\n- {}",
            error.errors.join("\n- ")
        ))
    })?;
    let kind = pack.has_collector().then(|| pack.kind().to_string());
    let snmp_profiles = pack.snmp_profiles().iter().map(|p| p.id.clone()).collect();
    let rules = pack.rules().iter().map(|rule| rule.uid.clone()).collect();
    let (status, install) = install_parsed(&state, pack, &actor, ip).await?;

    let file: String = body.filename.unwrap_or_default().chars().take(120).collect();
    let subject = if file.is_empty() {
        format!("{} ({})", conversion.pack_id, format_name(conversion.format))
    } else {
        format!("{} ({}, {file})", conversion.pack_id, format_name(conversion.format))
    };
    tracing::info!(actor = %actor, pack = %conversion.pack_id, "template imported");
    audit::record(&state.pool, Some(&actor), "template.imported", Some(&subject), ip).await;

    Ok((
        status,
        Json(Applied { pack_id: conversion.pack_id, kind, snmp_profiles, rules, install }),
    ))
}
