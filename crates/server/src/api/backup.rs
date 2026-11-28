//! Sauvegarde et restauration depuis l'interface.
//!
//! Quatre routes, toutes réservées à un administrateur **connecté** : un lot
//! contient en clair — pour qui a la phrase de passe — tous les identifiants du
//! parc. Un jeton d'API, même en écriture, ne l'obtient pas : un jeton vit dans
//! un script, et un script n'a pas à pouvoir exfiltrer l'instance entière.
//!
//! L'export est un `POST` et non un `GET` alors qu'il ne change rien : la phrase
//! de passe est dans le corps de la requête. Dans une URL, elle finirait dans
//! l'historique du navigateur, dans les journaux du mandataire inverse et dans
//! l'en-tête `Referer` de la page suivante.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Extension, Router};
use serde::{Deserialize, Serialize};

use crate::api::{ApiError, ApiResult};
use crate::auth::audit;
use crate::auth::client_ip::ClientIp;
use crate::auth::middleware::{CurrentPrincipal, Principal};
use crate::auth::users::User;
use crate::backup::{bundle, export, import, local};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/backup", get(status).post(create))
        .route("/backup/restore", post(restore))
        .route("/backup/local", post(run_local))
        .route("/backup/local/files", get(local_files))
        .route("/backup/local/files/{name}", get(download_local))
        .route("/backup/schedule", put(set_schedule))
        .merge(super::backup_destinations::routes())
}

/// Un lot n'est délivré qu'à un administrateur connecté dans l'interface.
pub(super) fn admin(principal: &Principal) -> ApiResult<&User> {
    match principal {
        Principal::User(user) if user.role.is_admin() => Ok(user),
        Principal::User(_) => {
            Err(ApiError::Forbidden("Only an administrator can export or restore a backup.".into()))
        }
        Principal::Token(_) => Err(ApiError::Forbidden(
            "A backup holds every credential of this instance, so it is only ever handed to an \
             administrator signed in to the web interface — never to an API token. Sign in and \
             use Settings → Backup."
                .into(),
        )),
    }
}

// --------------------------------------------------------------------------
// GET /api/backup — ce qu'il y a à sauvegarder et ce qui l'a été
// --------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Section {
    pub section: String,
    pub description: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct BackupStatus {
    /// Version de format écrite par ce serveur.
    pub bundle_version: u32,
    /// Longueur minimale exigée de la phrase de passe.
    pub min_passphrase_length: usize,
    /// Contenu du lot qui serait produit maintenant.
    pub contents: Vec<Section>,
    /// `file` quand le secret d'instance vit dans `secret.key`, `environment`
    /// quand il vient de `DUMBMONIT_SECRET`.
    pub secret_source: String,
    pub secret_path: String,
    pub schedule: local::Status,
}

pub async fn status(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
) -> ApiResult<Json<BackupStatus>> {
    admin(&principal)?;
    // Le lot est réellement construit : les décomptes annoncés sont ceux qui
    // partiront, et non une estimation qui se trouverait fausse le jour où une
    // section cesse d'être exportée.
    let collected =
        export::collect(&state.pool, &state.cipher, export::ExportOptions::default()).await?;
    let counts = collected.summary();
    let settings = local::Settings::load(&state.pool, &state.config).await;

    Ok(Json(BackupStatus {
        bundle_version: bundle::VERSION,
        min_passphrase_length: bundle::MIN_PASSPHRASE_LEN,
        contents: export::sections()
            .into_iter()
            .map(|(section, description)| Section {
                count: counts.get(section).copied().unwrap_or(0),
                section: section.to_string(),
                description: description.to_string(),
            })
            .collect(),
        secret_source: if state.config.secret.is_some() { "environment" } else { "file" }
            .to_string(),
        secret_path: state.config.secret_path().display().to_string(),
        schedule: local::status(&state.pool, &settings).await?,
    }))
}

// --------------------------------------------------------------------------
// POST /api/backup — produire le lot
// --------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ExportPayload {
    pub passphrase: String,
    /// Emporter aussi les empreintes de mots de passe, les secrets TOTP et les
    /// codes de secours des comptes. Faux par défaut.
    #[serde(default)]
    pub include_account_secrets: bool,
    /// Emporter aussi les versions des configurations réseau, qui contiennent
    /// les secrets des équipements. Faux par défaut.
    #[serde(default)]
    pub include_netconfig_versions: bool,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
    ClientIp(ip): ClientIp,
    Json(payload): Json<ExportPayload>,
) -> ApiResult<Response> {
    let me = admin(&principal)?.username.clone();
    bundle::check_passphrase(&payload.passphrase)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let options = export::ExportOptions {
        include_account_secrets: payload.include_account_secrets,
        include_netconfig_versions: payload.include_netconfig_versions,
    };
    let collected = export::collect(&state.pool, &state.cipher, options).await?;
    // Argon2id (32 Mio, trois passes) hors des fils du runtime : une dérivation
    // ne doit pas figer les autres requêtes.
    let passphrase = payload.passphrase.clone();
    let envelope =
        tokio::task::spawn_blocking(move || bundle::seal(&collected, &passphrase)).await??;
    let body = serde_json::to_vec_pretty(&envelope)?;

    let name = format!("dumbmonit-backup-{}.json", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
    tracing::info!(
        actor = %me,
        accounts = payload.include_account_secrets,
        netconfig_versions = payload.include_netconfig_versions,
        "backup exported"
    );
    audit::record(&state.pool, Some(&me), "backup.exported", Some(&name), ip).await;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
            // Un lot n'a rien à faire dans un cache, ni du navigateur ni d'un
            // mandataire.
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        body,
    )
        .into_response())
}

// --------------------------------------------------------------------------
// POST /api/backup/restore — simuler, puis appliquer
// --------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RestorePayload {
    /// L'enveloppe telle qu'elle est dans le fichier.
    pub bundle: serde_json::Value,
    pub passphrase: String,
    /// Faux — le défaut — : simulation. Le compte rendu dit ce qui se passerait,
    /// rien n'est écrit.
    #[serde(default)]
    pub apply: bool,
}

pub async fn restore(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
    ClientIp(ip): ClientIp,
    Json(payload): Json<RestorePayload>,
) -> ApiResult<Json<import::RestoreReport>> {
    let me = admin(&principal)?.username.clone();

    let envelope: bundle::Envelope =
        crate::untrusted::from_value(payload.bundle).map_err(|_| {
            ApiError::BadRequest(
                "This file is not a DumbMonit backup: its header could not be read.".into(),
            )
        })?;
    // Format, version et algorithme d'abord : inutile de faire saisir une
    // phrase de passe pour un fichier que nous refuserons de toute façon.
    bundle::check_envelope(&envelope).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let passphrase = payload.passphrase.clone();
    let opened = tokio::task::spawn_blocking(move || bundle::open(&envelope, &passphrase))
        .await?
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let report = import::restore(&state.pool, &state.cipher, &opened, payload.apply).await?;

    if payload.apply {
        // Une sauvegarde d'avant les paquets communautaires porte encore les
        // anciens types (`meraki`…) : ses cibles prennent le type du paquet.
        if let Err(error) = crate::packs::convert_moved_kinds(&state.pool).await {
            tracing::warn!(%error, "devices of moved types not converted after the restore");
        }
        // Les paquets restaurés définissent des types : le registre les prend
        // tout de suite, sans attendre un redémarrage.
        if let Err(error) = crate::packs::sync_registry(&state.pool, &state.collectors).await {
            tracing::warn!(%error, "integration packs not reloaded after the restore");
        }
        tracing::info!(
            actor = %me,
            created = report.created,
            updated = report.updated,
            skipped = report.skipped,
            "backup restored"
        );
        audit::record(
            &state.pool,
            Some(&me),
            "backup.restored",
            Some(&format!("{} created, {} updated", report.created, report.updated)),
            ip,
        )
        .await;
    }
    Ok(Json(report))
}

// --------------------------------------------------------------------------
// POST /api/backup/local — écrire une sauvegarde locale tout de suite
// --------------------------------------------------------------------------

pub async fn run_local(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
    ClientIp(ip): ClientIp,
) -> ApiResult<Json<local::Status>> {
    let me = admin(&principal)?.username.clone();
    let settings = local::Settings::load(&state.pool, &state.config).await;
    let record = local::run_once(&state.pool, &settings).await;
    local::publish(&state).await;
    // Les copies hors site partent en tâche de fond ; `GET /backup/destinations`
    // dit quand elles sont faites.
    crate::backup::offsite::after_local_backup(&state, &settings, &record).await;
    audit::record(&state.pool, Some(&me), "backup.local_run", Some(&record.file), ip).await;
    Ok(Json(local::status(&state.pool, &settings).await?))
}

// --------------------------------------------------------------------------
// PUT /api/backup/schedule — la planification, depuis l'interface
// --------------------------------------------------------------------------

/// Les trois réglages de la planification. Ce que l'environnement impose
/// (`DUMBMONIT_BACKUP_*`) l'emporte, et la réponse le dit (`env_managed`).
#[derive(Debug, Deserialize)]
pub struct SchedulePayload {
    pub enabled: bool,
    pub interval_hours: u64,
    pub keep: usize,
}

pub async fn set_schedule(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
    ClientIp(ip): ClientIp,
    Json(payload): Json<SchedulePayload>,
) -> ApiResult<Json<local::Status>> {
    let me = admin(&principal)?.username.clone();
    if payload.interval_hours == 0 || payload.keep == 0 {
        return Err(ApiError::BadRequest(
            "The interval and the number of backups to keep must be at least 1.".into(),
        ));
    }
    let stored = local::ScheduleOverride {
        enabled: Some(payload.enabled),
        interval_hours: Some(payload.interval_hours),
        keep: Some(payload.keep),
    }
    .clamped();
    local::save_override(&state.pool, &stored).await?;
    let subject = format!(
        "{}, every {} h, keep {}",
        if payload.enabled { "on" } else { "off" },
        payload.interval_hours.clamp(1, 8760),
        payload.keep.clamp(1, 365)
    );
    audit::record(&state.pool, Some(&me), "settings.backup", Some(&subject), ip).await;
    let settings = local::Settings::load(&state.pool, &state.config).await;
    Ok(Json(local::status(&state.pool, &settings).await?))
}

// --------------------------------------------------------------------------
// GET /api/backup/local/files[/{name}] — les copies locales
// --------------------------------------------------------------------------

pub async fn local_files(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
) -> ApiResult<Json<Vec<local::BackupFile>>> {
    admin(&principal)?;
    let settings = local::Settings::load(&state.pool, &state.config).await;
    Ok(Json(local::list_files(&settings.directory).await?))
}

/// Télécharge une copie locale (la base SQLite seule, jamais `secret.key`).
/// Le nom est validé strictement avant toute lecture : voir [`local::is_backup_name`].
pub async fn download_local(
    State(state): State<AppState>,
    Extension(CurrentPrincipal(principal)): Extension<CurrentPrincipal>,
    ClientIp(ip): ClientIp,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    let me = admin(&principal)?.username.clone();
    if !local::is_backup_name(&name) {
        return Err(ApiError::BadRequest("This is not the name of a local backup.".into()));
    }
    let settings = local::Settings::load(&state.pool, &state.config).await;
    let body = match tokio::fs::read(settings.directory.join(&name)).await {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ApiError::NotFound(format!("No local backup named {name}.")));
        }
        Err(error) => return Err(anyhow::Error::from(error).into()),
    };
    audit::record(&state.pool, Some(&me), "backup.downloaded", Some(&name), ip).await;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/vnd.sqlite3".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        body,
    )
        .into_response())
}
