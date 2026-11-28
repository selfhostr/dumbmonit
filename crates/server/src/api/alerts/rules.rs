//! Règles d'alerte et surcharges par équipement.

use super::*;

// --------------------------------------------------------------------------
// Règles
// --------------------------------------------------------------------------

pub async fn list_rules(
    TeamAccess(access): TeamAccess,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<RuleView>>> {
    let rules = db::alerts::list_rules(&state.pool).await?;
    let mut views =
        with_overrides(&state.pool, rules.into_iter().map(RuleView::from).collect()).await?;
    // Compte restreint par ses équipes : les règles restent lisibles (ce sont
    // des réglages, pas des équipements), mais leurs références à des
    // équipements hors de ses dossiers disparaissent, et une règle qui ne
    // visait que ceux-là aussi.
    if let Some(visible) = access.visible_targets(&state.pool).await? {
        views.retain_mut(|view| {
            view.overrides.retain(|o| visible.contains(&o.target_id));
            view.channels.clear();
            match &mut view.selector {
                TargetSelector::Ids { ids } => {
                    ids.retain(|id| visible.contains(id));
                    !ids.is_empty()
                }
                _ => true,
            }
        });
    }
    // Mode MSP : les règles de l'instance et celles de ses clients seulement.
    clients::retain_visible(&state.pool, &access, clients::Kind::Rule, &mut views, |v| v.id, true)
        .await?;
    Ok(Json(views))
}

pub async fn create_rule(
    TeamAccess(access): TeamAccess,
    State(state): State<AppState>,
    Json(payload): Json<RulePayload>,
) -> ApiResult<(StatusCode, Json<RuleView>)> {
    let existing = db::alerts::list_rules(&state.pool).await?;
    let mut rule = payload.validate(&state, None, &existing).await?;
    rule.id = db::alerts::upsert_rule(&state.pool, &rule).await?;
    // Créée dans la vue d'un client : elle lui appartient.
    clients::adopt(&state.pool, &access, clients::Kind::Rule, rule.id).await?;
    Ok((StatusCode::CREATED, Json(view_with_overrides(&state.pool, rule).await?)))
}

pub async fn update_rule(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<RulePayload>,
) -> ApiResult<Json<RuleView>> {
    let existing = db::alerts::list_rules(&state.pool).await?;
    let current =
        existing.iter().find(|rule| rule.id == id).cloned().ok_or_else(|| rule_not_found(id))?;

    let mut rule = payload.validate(&state, Some(&current), &existing).await?;
    rule.id = db::alerts::upsert_rule(&state.pool, &rule).await?;
    // Désactiver par l'édition doit solder les alertes comme le bouton dédié,
    // sinon elles disparaîtraient au cycle suivant sans trace dans l'historique.
    if current.enabled && !rule.enabled {
        db::alerts::set_rule_enabled(&state.pool, rule.id, false).await?;
    }
    Ok(Json(view_with_overrides(&state.pool, rule).await?))
}

/// Remet une règle livrée à ses valeurs d'origine : seuils, durées, sévérité,
/// sélecteur, canaux, paramètres et activation. Ses surcharges par équipement
/// sont des réglages à part et restent en place.
pub async fn reset_rule(
    State(state): State<AppState>,
    AdminIdentity(principal): AdminIdentity,
    ClientIp(ip): ClientIp,
    Path(id): Path<i64>,
) -> ApiResult<Json<RuleView>> {
    let current = rule_by_id(&state.pool, id).await?;
    if !current.builtin {
        return Err(ApiError::Conflict(format!(
            "The rule \"{}\" was created here: only a rule that ships with DumbMonit has \
             default values to go back to.",
            current.name
        )));
    }
    let mut rule = crate::alerting::rules::builtin_rules()
        .into_iter()
        .find(|rule| rule.uid == current.uid)
        .ok_or_else(|| {
            ApiError::Conflict(format!(
                "The rule \"{}\" no longer ships with this version: there is no default to \
                 restore.",
                current.name
            ))
        })?;
    rule.id = current.id;
    rule.builtin = true;
    db::alerts::upsert_rule(&state.pool, &rule).await?;
    // Désactivée par la remise à zéro : on solde les alertes comme le bouton dédié.
    if current.enabled && !rule.enabled {
        db::alerts::set_rule_enabled(&state.pool, rule.id, false).await?;
    }
    audit::record(&state.pool, Some(&principal.label()), "alert_rule.reset", Some(&rule.uid), ip)
        .await;
    Ok(Json(view_with_overrides(&state.pool, rule).await?))
}

/// Supprime une règle créée par l'utilisateur.
///
/// Les règles livrées sont explicitement protégées : les effacer laisserait
/// l'instance sans aucune surveillance, et le seul retour en arrière possible
/// serait un redémarrage du serveur, qui les réinsère. La désactivation couvre le
/// besoin réel — « je ne veux plus de cette alerte » — sans cet effet de bord.
pub async fn delete_rule(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let rules = db::alerts::list_rules(&state.pool).await?;
    let rule = rules.iter().find(|rule| rule.id == id).ok_or_else(|| rule_not_found(id))?;

    if rule.builtin {
        return Err(ApiError::Conflict(format!(
            "The rule \"{}\" ships with DumbMonit and cannot be deleted. Disable it \
             (POST /api/alerts/rules/{id}/enable with {{\"enabled\": false}}) or adjust \
             its threshold.",
            rule.name
        )));
    }

    if db::alerts::delete_rule(&state.pool, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(rule_not_found(id))
    }
}

pub async fn set_rule_enabled(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<EnablePayload>,
) -> ApiResult<Json<RuleView>> {
    if !db::alerts::set_rule_enabled(&state.pool, id, payload.enabled).await? {
        return Err(rule_not_found(id));
    }
    let rules = db::alerts::list_rules(&state.pool).await?;
    let rule = rules.into_iter().find(|rule| rule.id == id).ok_or_else(|| rule_not_found(id))?;
    Ok(Json(view_with_overrides(&state.pool, rule).await?))
}

// --------------------------------------------------------------------------
// Surcharges par équipement
// --------------------------------------------------------------------------

pub(super) async fn rule_by_id(pool: &sqlx::SqlitePool, id: i64) -> ApiResult<Rule> {
    db::alerts::list_rules(pool)
        .await?
        .into_iter()
        .find(|rule| rule.id == id)
        .ok_or_else(|| rule_not_found(id))
}

/// Surcharges d'une règle.
pub async fn list_overrides(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<RuleOverride>>> {
    let rule = rule_by_id(&state.pool, id).await?;
    Ok(Json(policy_store::list_overrides(&state.pool, Some(&rule.uid), None).await?))
}

/// Toutes les surcharges, éventuellement pour un seul équipement — c'est la
/// vue qu'une page d'équipement affiche.
pub async fn list_all_overrides(
    State(state): State<AppState>,
    Query(query): Query<OverridesQuery>,
) -> ApiResult<Json<Vec<RuleOverride>>> {
    Ok(Json(policy_store::list_overrides(&state.pool, None, query.target_id).await?))
}

/// Pose (ou remplace) la surcharge d'une règle pour un équipement.
pub async fn put_override(
    State(state): State<AppState>,
    OperatorIdentity(principal): OperatorIdentity,
    Path((id, target_id)): Path<(i64, i64)>,
    Json(mut payload): Json<OverridePayload>,
) -> ApiResult<Json<RuleOverride>> {
    // Un opérateur peut « ignorer » une règle pour un équipement, rien de plus :
    // un seuil est de la configuration. Les seuils déjà surchargés par un
    // administrateur sont conservés tels quels.
    let rule = rule_by_id(&state.pool, id).await?;
    if !principal.is_admin() {
        if !payload.is_ignore() {
            return Err(operator_override_refused());
        }
        if let Some(current) = current_override(&state, &rule.uid, target_id).await? {
            payload.threshold = current.threshold;
            payload.clear_threshold = current.clear_threshold;
        }
    }
    if !target_exists(&state, target_id).await? {
        return Err(ApiError::BadRequest(format!("Monitor {target_id} does not exist.")));
    }
    if let Some(threshold) = payload.threshold
        && !threshold.is_finite()
    {
        return Err(ApiError::BadRequest("The threshold must be a finite number.".into()));
    }
    let effective_threshold = payload.threshold.unwrap_or(rule.threshold);
    check_clear_threshold(rule.operator, effective_threshold, payload.clear_threshold)?;

    let over = RuleOverride {
        rule_uid: rule.uid,
        target_id,
        threshold: payload.threshold,
        clear_threshold: payload.clear_threshold,
        enabled: payload.enabled,
    };
    policy_store::upsert_override(&state.pool, &over).await?;
    Ok(Json(over))
}

pub async fn delete_override(
    State(state): State<AppState>,
    OperatorIdentity(principal): OperatorIdentity,
    Path((id, target_id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    let rule = rule_by_id(&state.pool, id).await?;
    // Un opérateur ne lève qu'un « ignorer » : les seuils surchargés par un
    // administrateur restent en place, seul `enabled` est retiré.
    if !principal.is_admin()
        && let Some(current) = current_override(&state, &rule.uid, target_id).await?
        && (current.threshold.is_some() || current.clear_threshold.is_some())
    {
        if current.enabled.is_none() {
            return Err(ApiError::NotFound(format!(
                "Rule {id} is not ignored for monitor {target_id}."
            )));
        }
        policy_store::upsert_override(&state.pool, &RuleOverride { enabled: None, ..current })
            .await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    if policy_store::delete_override(&state.pool, &rule.uid, target_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("No override of rule {id} for monitor {target_id}.")))
    }
}

pub(super) async fn current_override(
    state: &AppState,
    rule_uid: &str,
    target_id: i64,
) -> ApiResult<Option<RuleOverride>> {
    Ok(policy_store::list_overrides(&state.pool, Some(rule_uid), Some(target_id))
        .await?
        .into_iter()
        .next())
}

pub(super) fn operator_override_refused() -> ApiError {
    ApiError::Forbidden(
        "Operators can only ignore a rule for a device or undo that; changing thresholds \
         requires the admin role."
            .into(),
    )
}

pub(super) async fn target_exists(state: &AppState, target_id: i64) -> ApiResult<bool> {
    Ok(sqlx::query("SELECT 1 FROM targets WHERE id = ?")
        .bind(target_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(anyhow::Error::from)?
        .is_some())
}

/// Un seuil de retour doit être du côté « calme » du seuil : en dessous pour
/// « > » ou « ≥ », au-dessus pour « < » ou « ≤ ». Sinon l'alerte ne se
/// résoudrait jamais, ou l'hystérésis ne servirait à rien.
pub(super) fn check_clear_threshold(
    operator: Operator,
    threshold: f64,
    clear: Option<f64>,
) -> ApiResult<()> {
    let Some(clear) = clear else { return Ok(()) };
    if !clear.is_finite() {
        return Err(ApiError::BadRequest("The clear threshold must be a finite number.".into()));
    }
    let ok = match operator {
        Operator::Gt | Operator::Ge => clear < threshold,
        Operator::Lt | Operator::Le => clear > threshold,
    };
    if ok {
        Ok(())
    } else {
        Err(ApiError::BadRequest(match operator {
            Operator::Gt | Operator::Ge => format!(
                "The clear threshold ({clear}) must be below the threshold ({threshold}): the \
                 alert fires above the threshold and clears once the value drops under the \
                 clear threshold."
            ),
            Operator::Lt | Operator::Le => format!(
                "The clear threshold ({clear}) must be above the threshold ({threshold}): the \
                 alert fires below the threshold and clears once the value rises over the \
                 clear threshold."
            ),
        }))
    }
}

impl RulePayload {
    /// Traduit la saisie en règle du domaine, ou explique ce qui cloche.
    ///
    /// `current` porte la règle existante lors d'une modification : c'est d'elle que
    /// proviennent l'`uid` et le drapeau `builtin`, jamais du corps de la requête.
    pub(crate) async fn validate(
        self,
        state: &AppState,
        current: Option<&Rule>,
        existing: &[Rule],
    ) -> ApiResult<Rule> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err(ApiError::BadRequest("Rule name is required.".into()));
        }

        let query = self.query.trim().to_string();
        if query.is_empty() {
            return Err(ApiError::BadRequest(
                "The MetricsQL query is required: without it, the rule watches nothing.".into(),
            ));
        }

        let kind = parse_kind(self.kind.as_deref())?;
        let operator = parse_operator(self.operator.as_deref())?;
        let severity = parse_severity(self.severity.as_deref())?;

        let threshold = self.threshold.unwrap_or(0.0);
        if !threshold.is_finite() {
            return Err(ApiError::BadRequest(
                "The threshold must be a finite number: neither NaN nor infinity can be \
                 compared to a measurement."
                    .into(),
            ));
        }
        // L'hystérésis n'a de sens que pour une comparaison à un seuil : une règle
        // d'anomalie se juge sur un score, pas sur la valeur.
        let clear_threshold = if kind == RuleKind::Anomaly { None } else { self.clear_threshold };
        check_clear_threshold(operator, threshold, clear_threshold)?;

        let for_duration = parse_duration(self.for_secs, "The hold duration \"for_secs\"")?
            .unwrap_or(Duration::ZERO);
        let repeat_interval =
            parse_duration(self.repeat_secs, "The reminder interval \"repeat_secs\"")?;
        let escalate_after = parse_duration(
            self.escalate_after_secs,
            "The escalation delay \"escalate_after_secs\"",
        )?;

        if kind == RuleKind::LogPattern && crate::logs::central::has_pipe(&query) {
            return Err(ApiError::BadRequest(
                "A log pattern accepts filters only: remove the `|` pipes from the query.".into(),
            ));
        }
        let selector = parse_selector(self.selector)?;
        let params = parse_params(self.params)?;
        let channels = validate_channels(state, self.channels.unwrap_or_default()).await?;
        let uid = resolve_uid(self.uid.as_deref(), &name, current, existing)?;

        Ok(Rule {
            id: current.map_or(0, |rule| rule.id),
            uid,
            name,
            description: self.description.unwrap_or_default(),
            kind,
            query,
            operator,
            threshold,
            clear_threshold,
            for_duration,
            severity,
            selector,
            channels,
            params,
            unit: self.unit.unwrap_or_default(),
            repeat_interval,
            escalate_after,
            enabled: self.enabled.or(current.map(|rule| rule.enabled)).unwrap_or(true),
            // Le drapeau ne peut pas être posé depuis l'API : il désigne ce que le
            // produit livre, pas ce que l'utilisateur écrit.
            builtin: current.is_some_and(|rule| rule.builtin),
        })
    }
}

pub(super) fn parse_kind(raw: Option<&str>) -> ApiResult<RuleKind> {
    match raw.map(str::trim) {
        None | Some("") | Some("threshold") => Ok(RuleKind::Threshold),
        Some("anomaly") => Ok(RuleKind::Anomaly),
        Some("predict") => Ok(RuleKind::Predict),
        Some("log_pattern") => Ok(RuleKind::LogPattern),
        Some("forecast") => Ok(RuleKind::Forecast),
        Some("drift") => Ok(RuleKind::Drift),
        Some("slo_burn") => Ok(RuleKind::SloBurn),
        Some(other) => Err(ApiError::BadRequest(format!(
            "Unknown rule type \"{other}\" (expected: threshold, anomaly, predict, log_pattern, \
             forecast, drift, slo_burn)."
        ))),
    }
}

pub(super) fn parse_operator(raw: Option<&str>) -> ApiResult<Operator> {
    match raw.map(str::trim) {
        None | Some("") | Some(">") => Ok(Operator::Gt),
        Some(">=") => Ok(Operator::Ge),
        Some("<") => Ok(Operator::Lt),
        Some("<=") => Ok(Operator::Le),
        Some(other) => Err(ApiError::BadRequest(format!(
            "Invalid operator \"{other}\" (expected: >, >=, <, <=)."
        ))),
    }
}

pub(super) fn parse_severity(raw: Option<&str>) -> ApiResult<Severity> {
    match raw.map(str::trim) {
        None | Some("") | Some("warning") => Ok(Severity::Warning),
        Some("info") => Ok(Severity::Info),
        Some("critical") => Ok(Severity::Critical),
        Some(other) => Err(ApiError::BadRequest(format!(
            "Unknown severity \"{other}\" (expected: info, warning, critical)."
        ))),
    }
}

/// Durée en secondes, refusée si négative. Zéro équivaut à « pas de durée ».
pub(super) fn parse_duration(secs: Option<i64>, label: &str) -> ApiResult<Option<Duration>> {
    match secs {
        None => Ok(None),
        Some(value) if value < 0 => {
            Err(ApiError::BadRequest(format!("{label} cannot be negative.")))
        }
        Some(0) => Ok(None),
        Some(value) => Ok(Some(Duration::from_secs(value as u64))),
    }
}

pub(super) fn parse_selector(raw: Option<Value>) -> ApiResult<TargetSelector> {
    let Some(value) = raw.filter(|value| !value.is_null()) else {
        return Ok(TargetSelector::All);
    };
    // Le détail de serde est en anglais et cite des noms de variantes Rust : on lui
    // substitue les trois formes que l'utilisateur a effectivement le droit d'écrire.
    crate::untrusted::from_value(value).map_err(|_| {
        ApiError::BadRequest(
            "Invalid monitor selector. Accepted forms: {\"kind\":\"all\"}, \
             {\"kind\":\"ids\",\"ids\":[1,2]} or \
             {\"kind\":\"labels\",\"labels\":{\"role\":\"nas\"}}."
                .into(),
        )
    })
}

/// Bornes des réglages de prévision et de dérive.
pub(super) fn check_derived_params(params: &AnomalyParams) -> ApiResult<()> {
    use crate::alerting::model::{
        MAX_DRIFT_WEEKS, MAX_FORECAST_WINDOW_DAYS, MIN_FORECAST_WINDOW_DAYS,
    };

    let days = params.forecast_window_days;
    if !(MIN_FORECAST_WINDOW_DAYS..=MAX_FORECAST_WINDOW_DAYS).contains(&days) {
        return Err(ApiError::BadRequest(format!(
            "\"forecast_window_days\" must be between {MIN_FORECAST_WINDOW_DAYS} and \
             {MAX_FORECAST_WINDOW_DAYS} days."
        )));
    }
    let (warning, critical) = (params.forecast_warning_days, params.forecast_critical_days);
    if !(warning.is_finite() && critical.is_finite() && critical > 0.0 && warning >= critical) {
        return Err(ApiError::BadRequest(
            "\"forecast_critical_days\" must be positive and \"forecast_warning_days\" at \
             least as large: the alert turns warning first, then critical."
                .into(),
        ));
    }
    if !(params.forecast_min_confidence.is_finite()
        && (0.0..=1.0).contains(&params.forecast_min_confidence))
    {
        return Err(ApiError::BadRequest(
            "\"forecast_min_confidence\" must be between 0 and 1.".into(),
        ));
    }
    if !params.forecast_limit.is_finite() {
        return Err(ApiError::BadRequest("\"forecast_limit\" must be a finite number.".into()));
    }
    if !(60..=86_400).contains(&params.drift_window_secs) {
        return Err(ApiError::BadRequest(
            "\"drift_window_secs\" must be between 60 and 86400 seconds.".into(),
        ));
    }
    if params.drift_weeks > MAX_DRIFT_WEEKS {
        return Err(ApiError::BadRequest(format!(
            "\"drift_weeks\" cannot exceed {MAX_DRIFT_WEEKS}."
        )));
    }
    let floors = [params.drift_min_value, params.drift_min_delta];
    if floors.iter().any(|v| !(v.is_finite() && *v >= 0.0)) {
        return Err(ApiError::BadRequest(
            "\"drift_min_value\" and \"drift_min_delta\" must be zero or positive.".into(),
        ));
    }
    Ok(())
}

pub(super) fn parse_params(raw: Option<Value>) -> ApiResult<AnomalyParams> {
    let Some(value) = raw.filter(|value| !value.is_null()) else {
        return Ok(AnomalyParams::default());
    };

    let payload: AnomalyParamsPayload = crate::untrusted::from_value(value).map_err(|_| {
        ApiError::BadRequest(
            "Invalid anomaly settings. \"k\", \"alpha\", \"mad_floor_abs\" and \
             \"mad_floor_rel\" must be numbers, \"min_samples\" a positive integer."
                .into(),
        )
    })?;

    let defaults = AnomalyParams::default();
    let params = AnomalyParams {
        k: payload.k.unwrap_or(defaults.k),
        alpha: payload.alpha.unwrap_or(defaults.alpha),
        mad_floor_abs: payload.mad_floor_abs.unwrap_or(defaults.mad_floor_abs),
        mad_floor_rel: payload.mad_floor_rel.unwrap_or(defaults.mad_floor_rel),
        min_samples: payload.min_samples.unwrap_or(defaults.min_samples),
        log_window_secs: payload.window_secs.unwrap_or(defaults.log_window_secs),
        log_per_target: payload.per_target.unwrap_or(defaults.log_per_target),
        forecast_window_days: payload.forecast_window_days.unwrap_or(defaults.forecast_window_days),
        forecast_warning_days: payload
            .forecast_warning_days
            .unwrap_or(defaults.forecast_warning_days),
        forecast_critical_days: payload
            .forecast_critical_days
            .unwrap_or(defaults.forecast_critical_days),
        forecast_min_confidence: payload
            .forecast_min_confidence
            .unwrap_or(defaults.forecast_min_confidence),
        forecast_limit: payload.forecast_limit.unwrap_or(defaults.forecast_limit),
        drift_window_secs: payload.drift_window_secs.unwrap_or(defaults.drift_window_secs),
        drift_weeks: payload.drift_weeks.unwrap_or(defaults.drift_weeks),
        drift_min_samples: payload.drift_min_samples.unwrap_or(defaults.drift_min_samples),
        drift_min_value: payload.drift_min_value.unwrap_or(defaults.drift_min_value),
        drift_min_delta: payload.drift_min_delta.unwrap_or(defaults.drift_min_delta),
        drift_direction: payload.drift_direction.unwrap_or(defaults.drift_direction),
    };
    check_derived_params(&params)?;
    if !(crate::alerting::model::MIN_LOG_WINDOW_SECS..=crate::alerting::model::MAX_LOG_WINDOW_SECS)
        .contains(&params.log_window_secs)
    {
        return Err(ApiError::BadRequest(format!(
            "\"window_secs\" must be between {} and {} seconds.",
            crate::alerting::model::MIN_LOG_WINDOW_SECS,
            crate::alerting::model::MAX_LOG_WINDOW_SECS
        )));
    }

    if !(params.k.is_finite() && params.k > 0.0) {
        return Err(ApiError::BadRequest(
            "\"k\" must be a strictly positive number: it is the number of robust \
             deviations beyond which a point is considered abnormal."
                .into(),
        ));
    }
    if !(params.alpha.is_finite() && params.alpha > 0.0 && params.alpha <= 1.0) {
        return Err(ApiError::BadRequest(
            "\"alpha\" must be between 0 (exclusive) and 1 (inclusive).".into(),
        ));
    }
    if !(params.mad_floor_abs.is_finite() && params.mad_floor_abs > 0.0) {
        return Err(ApiError::BadRequest(
            "\"mad_floor_abs\" must be strictly positive: without a floor, a perfectly \
             flat series would score any deviation as infinite."
                .into(),
        ));
    }
    if !(params.mad_floor_rel.is_finite() && params.mad_floor_rel >= 0.0) {
        return Err(ApiError::BadRequest("\"mad_floor_rel\" cannot be negative.".into()));
    }
    if params.min_samples == 0 {
        return Err(ApiError::BadRequest(
            "\"min_samples\" must be at least 1: scoring an empty bucket makes no sense.".into(),
        ));
    }
    Ok(params)
}

/// Vérifie que chaque canal destinataire existe, et retire les doublons.
///
/// Un identifiant fantôme rendrait la règle muette sans le moindre message : le
/// moteur n'enverrait rien puisqu'aucun canal ne correspondrait, et l'utilisateur
/// conclurait que l'alerting est en panne.
pub(super) async fn validate_channels(state: &AppState, wanted: Vec<i64>) -> ApiResult<Vec<i64>> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }

    let known = channels::existing_ids(&state.pool).await?;
    let mut seen = HashSet::new();
    // Pas de préallocation : la taille vient du client.
    let mut channels = Vec::new();
    for id in wanted {
        if !known.contains(&id) {
            return Err(ApiError::BadRequest(format!(
                "Notification channel {id} does not exist. Create it first, or leave \
                 \"channels\" empty to notify every enabled channel."
            )));
        }
        if seen.insert(id) {
            channels.push(id);
        }
    }
    Ok(channels)
}

/// Détermine l'identifiant stable d'une règle.
///
/// Il est immuable après création : c'est lui qui relie une règle à ses empreintes
/// et à son historique. Le renommer résoudrait toutes ses alertes en cours puis les
/// ferait renaître, ce que personne ne demande en corrigeant un libellé.
pub(super) fn resolve_uid(
    submitted: Option<&str>,
    name: &str,
    current: Option<&Rule>,
    existing: &[Rule],
) -> ApiResult<String> {
    if let Some(current) = current {
        return match submitted.map(str::trim).filter(|uid| !uid.is_empty()) {
            Some(uid) if uid != current.uid => Err(ApiError::BadRequest(format!(
                "The stable identifier of a rule cannot be changed (\"{}\" here): it links \
                 the rule to its history and its active alerts.",
                current.uid
            ))),
            _ => Ok(current.uid.clone()),
        };
    }

    let taken: HashSet<&str> = existing.iter().map(|rule| rule.uid.as_str()).collect();

    match submitted.map(str::trim).filter(|uid| !uid.is_empty()) {
        Some(uid) => {
            check_uid_shape(uid)?;
            if taken.contains(uid) {
                return Err(ApiError::Conflict(format!(
                    "A rule already uses the identifier \"{uid}\"."
                )));
            }
            Ok(uid.to_string())
        }
        None => {
            let base = slugify(name);
            if base.is_empty() {
                return Err(ApiError::BadRequest(
                    "Cannot derive an identifier from the name: add at least one letter \
                     or digit, or provide \"uid\" explicitly."
                        .into(),
                ));
            }
            check_uid_shape(&base)?;
            // Un suffixe numérique plutôt qu'un refus : l'utilisateur n'a pas choisi
            // cet identifiant, le lui reprocher n'aurait aucun sens.
            if !taken.contains(base.as_str()) {
                return Ok(base);
            }
            (2..1000)
                .map(|suffix| format!("{base}_{suffix}"))
                .find(|candidate| !taken.contains(candidate.as_str()))
                .ok_or_else(|| {
                    ApiError::Conflict(format!(
                        "Too many rules have a name close to \"{name}\". Provide \"uid\" \
                         explicitly."
                    ))
                })
        }
    }
}

pub(super) fn check_uid_shape(uid: &str) -> ApiResult<()> {
    if uid.len() > MAX_UID_LEN {
        return Err(ApiError::BadRequest(format!(
            "A rule identifier is limited to {MAX_UID_LEN} characters."
        )));
    }
    if !uid.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
        return Err(ApiError::BadRequest(format!(
            "The identifier \"{uid}\" must only contain unaccented lowercase letters, \
             digits, hyphens and underscores."
        )));
    }
    // `host_down` désigne nommément la règle qui définit « équipement injoignable »,
    // racine de la suppression par dépendance : la laisser porter autre chose
    // couperait toutes les alertes des équipements situés derrière.
    if uid == RULE_HOST_DOWN {
        return Err(ApiError::BadRequest(format!(
            "The identifier \"{RULE_HOST_DOWN}\" is reserved for the \"Monitor unreachable\" \
             rule, on which dependency suppression relies."
        )));
    }
    Ok(())
}

/// Identifiant lisible dérivé d'un nom saisi en français.
///
/// Les accents sont repliés plutôt que remplacés par un tiret bas : « Mémoire
/// saturée » doit donner `memoire_saturee`, pas `m_moire_satur_e`.
pub(super) fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for raw in name.chars() {
        let folded = fold_accent(raw);
        if folded.is_ascii_alphanumeric() {
            slug.push(folded.to_ascii_lowercase());
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug = slug.trim_matches('_').to_string();
    slug.chars().take(MAX_UID_LEN).collect()
}

pub(super) fn fold_accent(c: char) -> char {
    match c {
        'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'À' | 'Â' | 'Ä' | 'Á' | 'Ã' | 'Å' => 'A',
        'Ç' => 'C',
        'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'Ñ' => 'N',
        'Ò' | 'Ó' | 'Ô' | 'Ö' | 'Õ' => 'O',
        'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
        'Ý' => 'Y',
        other => other,
    }
}

pub(super) fn rule_not_found(id: i64) -> ApiError {
    ApiError::NotFound(format!("Alert rule {id} not found."))
}
