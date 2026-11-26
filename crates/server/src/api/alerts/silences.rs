//! Mises en sourdine.

use super::*;

// --------------------------------------------------------------------------
// Silences
// --------------------------------------------------------------------------

pub async fn list_silences(
    TeamAccess(access): TeamAccess,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<SilenceView>>> {
    let now = Utc::now();
    let silences = visible_silences(&access, &state).await?;
    Ok(Json(silences.into_iter().map(|s| SilenceView::new(s, now)).collect()))
}

/// Les silences que ce compte a le droit de voir.
pub(crate) async fn visible_silences(access: &Access, state: &AppState) -> ApiResult<Vec<Silence>> {
    let mut silences = db::alerts::list_silences(&state.pool).await?;
    // Compte restreint : seulement les silences posés sur ses équipements. La
    // vue d'un client par l'administrateur montre aussi les silences sans
    // équipement de ce client.
    if let Some(visible) = access.visible_targets(&state.pool).await? {
        let owners = clients::owners(&state.pool, clients::Kind::Silence).await?;
        let admin_view = !access.is_scoped();
        silences.retain(|s| match s.target_id {
            Some(id) => visible.contains(&id),
            None => admin_view && owners.get(&s.id).is_some_and(|c| access.sees_client(*c)),
        });
    }
    Ok(silences)
}

pub async fn create_silence(
    TeamAccess(access): TeamAccess,
    State(state): State<AppState>,
    Json(payload): Json<SilencePayload>,
) -> ApiResult<(StatusCode, Json<SilenceView>)> {
    // Avant la validation, qui dirait sinon si un équipement d'une autre
    // équipe existe.
    check_team_silence(&state, &access, payload.target_id).await?;
    let mut silence = payload.validate(&state).await?;
    silence.id = db::alerts::create_silence(&state.pool, &silence).await?;
    // Mode MSP : un silence sur un équipement suit son client ; sans
    // équipement, il appartient au client de la vue (sinon à l'instance).
    match silence.target_id {
        Some(target) => {
            let client = clients::target_client(&state.pool, target).await?;
            clients::assign(&state.pool, clients::Kind::Silence, &[silence.id], client)
                .await
                .map_err(crate::api::clients::api_error)?;
        }
        None => clients::adopt(&state.pool, &access, clients::Kind::Silence, silence.id).await?,
    }
    Ok((StatusCode::CREATED, Json(SilenceView::new(silence, Utc::now()))))
}

/// Un compte restreint par ses équipes ne pose un silence que sur un
/// équipement qu'il peut traiter : jamais sur toute l'instance ni sur de
/// simples étiquettes, qui couvriraient des équipements d'autres équipes.
pub(super) async fn check_team_silence(
    state: &AppState,
    access: &Access,
    target_id: Option<i64>,
) -> ApiResult<()> {
    if !access.is_scoped() {
        return Ok(());
    }
    let Some(target_id) = target_id else {
        return Err(ApiError::Forbidden(
            "Team-restricted accounts can only schedule maintenance for a single target.".into(),
        ));
    };
    match access.target_level(&state.pool, target_id).await? {
        None => Err(ApiError::NotFound("Target not found.".into())),
        Some(level) if level >= Level::Operate => Ok(()),
        Some(_) => Err(ApiError::Forbidden(
            "Your teams only let you view this target, not schedule maintenance for it.".into(),
        )),
    }
}

/// Modifie une fenêtre : nom, commentaire, horaires, portée. Même charge utile
/// et mêmes contrôles qu'à la création. Terminer une maintenance, c'est la
/// modifier pour que sa fin tombe à maintenant.
pub async fn update_silence(
    TeamAccess(access): TeamAccess,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<SilencePayload>,
) -> ApiResult<Json<SilenceView>> {
    let existing =
        db::alerts::list_silences(&state.pool)
            .await?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| ApiError::NotFound(format!("Maintenance window {id} not found.")))?;
    // Un compte restreint ne touche qu'aux fenêtres de ses équipements, et ne
    // les déplace que vers un équipement qu'il peut traiter.
    check_team_silence(&state, &access, existing.target_id).await?;
    check_team_silence(&state, &access, payload.target_id).await?;
    let mut silence = payload.validate(&state).await?;
    silence.id = id;
    if !db::alerts::update_silence(&state.pool, &silence).await? {
        return Err(ApiError::NotFound(format!("Maintenance window {id} not found.")));
    }
    if silence.target_id != existing.target_id
        && let Some(target) = silence.target_id
    {
        let client = clients::target_client(&state.pool, target).await?;
        clients::assign(&state.pool, clients::Kind::Silence, &[silence.id], client)
            .await
            .map_err(crate::api::clients::api_error)?;
    }
    Ok(Json(SilenceView::new(silence, Utc::now())))
}

pub async fn delete_silence(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if db::alerts::delete_silence(&state.pool, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("Maintenance window {id} not found.")))
    }
}

impl SilencePayload {
    pub(super) async fn validate(self, state: &AppState) -> ApiResult<Silence> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err(ApiError::BadRequest(
                "Maintenance window name is required: six months from now, it is what \
                 explains why these alerts are silent."
                    .into(),
            ));
        }

        if let Some(target_id) = self.target_id {
            let exists = sqlx::query("SELECT 1 FROM targets WHERE id = ?")
                .bind(target_id)
                .fetch_optional(&state.pool)
                .await
                .map_err(anyhow::Error::from)?
                .is_some();
            if !exists {
                return Err(ApiError::BadRequest(format!("Monitor {target_id} does not exist.")));
            }
        }

        let schedule = parse_schedule(self.schedule)?;
        // « Jusqu'au rétablissement » se lève quand une cible répond de nouveau : sans
        // cible, rien ne le lèverait jamais, ce serait un silence permanent.
        if matches!(schedule, Schedule::Recovery { .. }) && self.target_id.is_none() {
            return Err(ApiError::BadRequest(
                "A silence until recovery needs a monitor (\"target_id\").".into(),
            ));
        }

        Ok(Silence {
            id: 0,
            name,
            comment: self.comment.unwrap_or_default(),
            target_id: self.target_id,
            matchers: self.matchers,
            schedule,
            enabled: self.enabled.unwrap_or(true),
        })
    }
}

pub(crate) fn parse_schedule(raw: Value) -> ApiResult<Schedule> {
    let schedule: Schedule = crate::untrusted::from_value(raw).map_err(|_| {
        ApiError::BadRequest(
            "Invalid schedule. Accepted forms: \
             {\"kind\":\"once\",\"starts_at\":\"2026-01-01T00:00:00Z\",\
             \"ends_at\":\"2026-01-01T02:00:00Z\"}, \
             {\"kind\":\"recovery\",\"starts_at\":\"2026-01-01T00:00:00Z\"}, \
             {\"kind\":\"weekly\",\"days\":[6],\"start_minute\":120,\"end_minute\":240,\
             \"timezone\":\"Europe/Paris\"} or \
             {\"kind\":\"monthly\",\"nth_weekdays\":[{\"nth\":1,\"weekday\":6}],\
             \"start_minute\":120,\"duration_minutes\":120,\"timezone\":\"Europe/Paris\"}"
                .into(),
        )
    })?;

    match &schedule {
        Schedule::Once { starts_at, ends_at } => {
            if ends_at <= starts_at {
                return Err(ApiError::BadRequest(
                    "The end of the window must be after its start.".into(),
                ));
            }
        }
        // Rien à vérifier ici : `starts_at` est celui du client, et la cible est exigée
        // à la création (voir `SilenceInput::into_silence`).
        Schedule::Recovery { .. } => {}
        Schedule::Weekly { days, start_minute, end_minute, utc_offset_minutes, .. } => {
            if days.is_empty() {
                return Err(ApiError::BadRequest(
                    "Select at least one day of the week (0 = Monday … 6 = Sunday).".into(),
                ));
            }
            if let Some(day) = days.iter().find(|day| **day > 6) {
                return Err(ApiError::BadRequest(format!(
                    "Invalid day of the week \"{day}\": days go from 0 (Monday) to 6 (Sunday)."
                )));
            }
            for (label, minute) in [("start_minute", *start_minute), ("end_minute", *end_minute)] {
                if minute >= MINUTES_PER_DAY {
                    return Err(ApiError::BadRequest(format!(
                        "\"{label}\" must be between 0 and {} (minutes since midnight).",
                        MINUTES_PER_DAY - 1
                    )));
                }
            }
            // Début et fin confondus feraient une fenêtre couvrant la journée
            // entière — un silence permanent posé par accident, exactement ce qu'un
            // outil de supervision ne doit jamais faire en silence.
            if start_minute == end_minute {
                return Err(ApiError::BadRequest(
                    "The start and end of the window must differ. To silence an alert \
                     permanently, disable the rule instead."
                        .into(),
                ));
            }
            if !(MIN_UTC_OFFSET..=MAX_UTC_OFFSET).contains(utc_offset_minutes) {
                return Err(ApiError::BadRequest(format!(
                    "\"utc_offset_minutes\" must be between {MIN_UTC_OFFSET} and \
                     {MAX_UTC_OFFSET}."
                )));
            }
        }
        Schedule::Monthly {
            days,
            nth_weekdays,
            start_minute,
            duration_minutes,
            utc_offset_minutes,
            ..
        } => {
            if days.is_empty() && nth_weekdays.is_empty() {
                return Err(ApiError::BadRequest(
                    "A monthly window needs at least one day of the month (\"days\") or one \
                     weekday of the month (\"nth_weekdays\", for example the first Sunday)."
                        .into(),
                ));
            }
            if days.len() + nth_weekdays.len() > MAX_MONTHLY_ENTRIES {
                return Err(ApiError::BadRequest(format!(
                    "A monthly window is limited to {MAX_MONTHLY_ENTRIES} days."
                )));
            }
            if let Some(day) = days.iter().find(|day| !(1..=31).contains(*day)) {
                return Err(ApiError::BadRequest(format!(
                    "Invalid day of the month \"{day}\": days go from 1 to 31. A month \
                     without that day is simply skipped."
                )));
            }
            for nth in nth_weekdays {
                if nth.weekday > 6 {
                    return Err(ApiError::BadRequest(format!(
                        "Invalid weekday \"{}\": weekdays go from 0 (Monday) to 6 (Sunday).",
                        nth.weekday
                    )));
                }
                if !(1..=5).contains(&nth.nth) && nth.nth != LAST_OF_MONTH {
                    return Err(ApiError::BadRequest(format!(
                        "Invalid rank \"{}\": use 1 to 5 for the first to the fifth weekday \
                         of the month, or {LAST_OF_MONTH} for the last one.",
                        nth.nth
                    )));
                }
            }
            if *start_minute >= MINUTES_PER_DAY {
                return Err(ApiError::BadRequest(format!(
                    "\"start_minute\" must be between 0 and {} (minutes since midnight).",
                    MINUTES_PER_DAY - 1
                )));
            }
            if *duration_minutes == 0 || *duration_minutes > MAX_DURATION_MINUTES {
                return Err(ApiError::BadRequest(format!(
                    "\"duration_minutes\" must be between 1 and {MAX_DURATION_MINUTES} \
                     (thirty days). To silence an alert permanently, disable the rule instead."
                )));
            }
            if !(MIN_UTC_OFFSET..=MAX_UTC_OFFSET).contains(utc_offset_minutes) {
                return Err(ApiError::BadRequest(format!(
                    "\"utc_offset_minutes\" must be between {MIN_UTC_OFFSET} and \
                     {MAX_UTC_OFFSET}."
                )));
            }
        }
    }

    check_timezone(&schedule)?;
    Ok(schedule)
}

/// Refuse un nom de fuseau que le serveur ne saura pas résoudre.
///
/// Un fuseau inconnu ne casse rien — la fenêtre retombe sur son décalage fixe —
/// mais elle s'ouvrirait alors à une autre heure que celle que l'utilisateur
/// croit avoir posée, ce qui est pire qu'un refus immédiat.
pub(super) fn check_timezone(schedule: &Schedule) -> ApiResult<()> {
    let Some(name) = schedule.timezone() else { return Ok(()) };
    let name = name.trim();
    if name.is_empty() {
        return Ok(());
    }
    if name.parse::<chrono_tz::Tz>().is_err() {
        return Err(ApiError::BadRequest(format!(
            "Unknown time zone \"{name}\". Use an IANA name such as Europe/Paris, \
             America/New_York or UTC."
        )));
    }
    Ok(())
}
