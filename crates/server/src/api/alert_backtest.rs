//! `POST /alerts/rules/backtest` : « tester sur l'historique ».
//!
//! La règle arrive telle qu'elle est dans l'éditeur, enregistrée ou non ; elle
//! est validée comme pour un enregistrement, puis rejouée sur les mesures
//! stockées du dernier jour, des sept ou des trente derniers jours, au pas
//! d'évaluation du moteur (voir [`crate::alerting::replay`]). Rien n'est écrit.
//!
//! Le coût est borné de trois façons, et la réponse dit chaque fois quand une
//! borne a joué : au-delà de [`MAX_STEPS`] pas, le pas est élargi
//! (`coarsened`) ; au-delà de [`MAX_SERIES`] séries, seules les plus souvent
//! au-dessus du seuil sont rejouées (`truncated`) ; la lecture des mesures
//! est abandonnée après [`QUERY_TIMEOUT`].

use std::collections::HashMap;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::alerting::cycle::{TargetIndex, UNKNOWN_TARGET};
use crate::alerting::engine::AlertingConfig;
use crate::alerting::model::{self, Rule, TARGET_NAME_LABELS, TargetId};
use crate::alerting::overrides::{Effective, OverrideIndex, RuleOverride};
use crate::alerting::replay::{self, FiringPeriod, Grid, NotReplayable};
use crate::alerting::source::{MetricSource, SeriesRange};
use crate::api::alerts::RulePayload;
use crate::api::{ApiError, ApiResult};
use crate::auth::teams::{self, TeamAccess};
use crate::db;
use crate::notify::policy_store;
use crate::state::AppState;

/// Fenêtres proposées, en jours.
pub const WINDOWS_DAYS: [i64; 3] = [1, 7, 30];
const DEFAULT_DAYS: i64 = 7;
/// Pas au plus par série : trente jours au pas de 30 s en feraient 86 400.
pub const MAX_STEPS: u64 = 10_000;
/// Séries rejouées au plus.
pub const MAX_SERIES: usize = 100;
/// Délai accordé à VictoriaMetrics pour rendre la plage.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
pub struct BacktestPayload {
    /// La règle telle qu'elle est éditée, au format de `POST /alerts/rules`.
    pub rule: RulePayload,
    /// La règle enregistrée qu'on modifie : son identifiant stable et ses
    /// surcharges par équipement s'appliquent. Absent pour une règle neuve.
    #[serde(default)]
    pub rule_id: Option<i64>,
    /// 1, 7 ou 30. Sept par défaut.
    #[serde(default)]
    pub days: Option<i64>,
}

/// Le rejeu d'une série.
#[derive(Debug, Serialize)]
pub struct SeriesView {
    pub series_key: String,
    pub target_id: Option<TargetId>,
    pub target_name: String,
    /// Seuil appliqué à cette série, surcharge comprise.
    pub threshold: f64,
    pub clear_threshold: Option<f64>,
    pub periods: Vec<FiringPeriod>,
    pub notifications: u32,
    pub absorbed: u32,
    pub firing_secs: i64,
    /// Valeurs résumées sur la fenêtre (au plus 96), pour la miniature.
    pub spark: Vec<f64>,
}

#[derive(Debug, Serialize)]
pub struct BacktestView {
    /// Faux pour une règle qui ne se rejoue pas : `not_replayable` dit pourquoi.
    pub replayable: bool,
    pub not_replayable: Option<NotReplayable>,
    pub days: i64,
    /// Bornes de la fenêtre rejouée, en secondes Unix.
    pub start: i64,
    pub end: i64,
    /// Pas du rejeu, et pas d'évaluation du moteur : différents quand la
    /// fenêtre a imposé d'élargir le pas.
    pub step_secs: i64,
    pub interval_secs: i64,
    pub coarsened: bool,
    /// Séries rendues par la requête (après sélecteur et surcharges).
    pub series_total: usize,
    /// Vrai si toutes n'ont pas été rejouées.
    pub truncated: bool,
    /// Périodes de déclenchement, notifications et dépassements absorbés par
    /// `for`, toutes séries rejouées confondues.
    pub fired: u32,
    pub notifications: u32,
    pub absorbed: u32,
    /// Séries rejouées qui n'auraient jamais déclenché.
    pub quiet_series: usize,
    /// Séries qui auraient déclenché, les plus bruyantes d'abord.
    pub series: Vec<SeriesView>,
}

impl BacktestView {
    fn empty(days: i64, grid: Grid, interval_secs: i64) -> Self {
        Self {
            replayable: true,
            not_replayable: None,
            days,
            start: grid.start_s,
            end: grid.end_s,
            step_secs: grid.step_s,
            interval_secs,
            coarsened: grid.step_s > interval_secs,
            series_total: 0,
            truncated: false,
            fired: 0,
            notifications: 0,
            absorbed: 0,
            quiet_series: 0,
            series: Vec::new(),
        }
    }
}

/// Une série retenue, rangée sur la grille.
struct Candidate {
    series_key: String,
    target_id: Option<TargetId>,
    target_name: String,
    effective: Effective,
    values: Vec<Option<f64>>,
    /// Pas au-dessus du seuil : de quoi garder les séries qui comptent quand
    /// il faut couper.
    violations: usize,
}

pub async fn backtest(
    TeamAccess(access): TeamAccess,
    State(state): State<AppState>,
    Json(payload): Json<BacktestPayload>,
) -> ApiResult<Json<BacktestView>> {
    let days = payload.days.unwrap_or(DEFAULT_DAYS);
    if !WINDOWS_DAYS.contains(&days) {
        return Err(ApiError::BadRequest(format!(
            "\"days\" must be 1, 7 or 30 (received: {days})."
        )));
    }
    let existing = db::alerts::list_rules(&state.pool).await?;
    let current = match payload.rule_id {
        Some(id) => Some(find_rule(&existing, id)?),
        None => None,
    };
    let rule = payload.rule.validate(&state, current.as_ref(), &existing).await?;

    let interval_secs =
        i64::try_from(AlertingConfig::from_env().interval.as_secs()).unwrap_or(30).max(1);
    let grid = Grid::new(Utc::now().timestamp(), days * 86_400, interval_secs, MAX_STEPS);
    let mut view = BacktestView::empty(days, grid, interval_secs);

    if let Some(why) = replay::not_replayable(rule.kind) {
        view.replayable = false;
        view.not_replayable = Some(why);
        return Ok(Json(view));
    }

    // Un compte restreint ne rejoue que ses équipements.
    let visible = access.visible_targets(&state.pool).await?;
    let filter = teams::series_filter(visible.as_ref());
    let step = u64::try_from(grid.step_s).unwrap_or(1);
    let fetched = tokio::time::timeout(
        QUERY_TIMEOUT,
        state.victoria.range(&rule.query, grid.start_s, grid.end_s, step, filter.as_deref()),
    )
    .await;
    let ranges = match fetched {
        Err(_) => {
            return Err(ApiError::BadGateway(
                "The metrics backend did not answer in time: try a shorter window.".into(),
            ));
        }
        Ok(Err(error)) => return Err(backend_error(error)),
        Ok(Ok(ranges)) => ranges,
    };

    let targets = db::alerts::list_target_nodes(&state.pool).await?;
    let overrides = policy_store::list_overrides(&state.pool, Some(&rule.uid), None).await?;
    // Ranger chaque série sur la grille (dix mille pas, avant même la coupe à
    // cent séries) puis rejouer : du calcul pur, hors de la boucle d'axum.
    let (series_total, truncated, replayed) = tokio::task::spawn_blocking(move || {
        let mut candidates = candidates(&rule, &ranges, &targets, &overrides, grid);
        // Les points bruts ne servent plus : rendus avant le rejeu.
        drop(ranges);
        let total = candidates.len();
        let truncated = total > MAX_SERIES;
        if truncated {
            candidates.sort_by_key(|c| std::cmp::Reverse(c.violations));
            candidates.truncate(MAX_SERIES);
        }
        (total, truncated, replay_all(&rule, candidates, grid))
    })
    .await?;
    view.series_total = series_total;
    view.truncated = truncated;
    view.quiet_series = replayed.iter().filter(|series| series.periods.is_empty()).count();
    view.fired = replayed.iter().map(|series| series.periods.len() as u32).sum();
    view.notifications = replayed.iter().map(|series| series.notifications).sum();
    view.absorbed = replayed.iter().map(|series| series.absorbed).sum();
    view.series = replayed.into_iter().filter(|series| !series.periods.is_empty()).collect();
    Ok(Json(view))
}

fn find_rule(rules: &[Rule], id: i64) -> ApiResult<Rule> {
    rules
        .iter()
        .find(|rule| rule.id == id)
        .cloned()
        .ok_or_else(|| ApiError::NotFound(format!("Alert rule {id} not found.")))
}

/// Rattache chaque série à son équipement, écarte ce que le moteur écarterait
/// (équipement supprimé ou désactivé, hors sélecteur, règle désactivée pour
/// lui) et range ses points sur la grille. Deux séries de même identité (avant
/// et après un renommage) n'en font qu'une, comme dans le moteur.
fn candidates(
    rule: &Rule,
    ranges: &[SeriesRange],
    targets: &[model::TargetNode],
    overrides: &[RuleOverride],
    grid: Grid,
) -> Vec<Candidate> {
    let index = TargetIndex::new(targets);
    let overrides = OverrideIndex::new(overrides);
    let mut position: HashMap<String, usize> = HashMap::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for range in ranges {
        let target = index.resolve(&range.labels);
        if target.is_some_and(|t| !t.enabled)
            || (target.is_none() && model::carries_target_id(&range.labels))
            || !rule.selector.matches(target)
        {
            continue;
        }
        let effective = overrides.effective(rule, target.map(|t| t.id));
        if !effective.enabled {
            continue;
        }
        let fingerprint = model::fingerprint(&rule.uid, &model::identity_key(&range.labels));
        let at = *position.entry(fingerprint).or_insert_with(|| {
            let target_name = target
                .map(|t| t.name.clone())
                .or_else(|| TARGET_NAME_LABELS.iter().find_map(|l| range.labels.get(*l).cloned()))
                .or_else(|| range.labels.get(model::SLO_LABEL).cloned())
                .unwrap_or_else(|| UNKNOWN_TARGET.to_string());
            candidates.push(Candidate {
                series_key: model::series_key(&range.labels),
                target_id: target.map(|t| t.id),
                target_name,
                effective,
                values: vec![None; grid.steps()],
                violations: 0,
            });
            candidates.len() - 1
        });
        grid.align(&range.points, &mut candidates[at].values);
    }
    for candidate in &mut candidates {
        candidate.violations = candidate
            .values
            .iter()
            .flatten()
            .filter(|value| rule.operator.test(**value, candidate.effective.threshold))
            .count();
    }
    candidates
}

/// Rejoue chaque série et les range des plus bruyantes aux plus calmes.
fn replay_all(rule: &Rule, candidates: Vec<Candidate>, grid: Grid) -> Vec<SeriesView> {
    let mut series: Vec<SeriesView> = candidates
        .into_iter()
        .map(|candidate| {
            let replayed =
                replay::replay_series(rule, candidate.effective, &candidate.values, grid);
            SeriesView {
                series_key: candidate.series_key,
                target_id: candidate.target_id,
                target_name: candidate.target_name,
                threshold: candidate.effective.threshold,
                clear_threshold: candidate.effective.clear_threshold,
                periods: replayed.periods,
                notifications: replayed.notifications,
                absorbed: replayed.absorbed,
                firing_secs: replayed.firing_secs,
                spark: replay::spark(&candidate.values, rule.operator, replay::SPARK_POINTS),
            }
        })
        .collect();
    series.sort_by(|a, b| {
        b.notifications
            .cmp(&a.notifications)
            .then(b.periods.len().cmp(&a.periods.len()))
            .then(b.firing_secs.cmp(&a.firing_secs))
            .then_with(|| a.target_name.cmp(&b.target_name))
            .then_with(|| a.series_key.cmp(&b.series_key))
    });
    series
}

/// Un refus de VictoriaMetrics est une requête invalide (400) ; un backend
/// injoignable ou illisible est une panne côté serveur (502).
fn backend_error(error: anyhow::Error) -> ApiError {
    if error.downcast_ref::<crate::tsdb::QueryRejected>().is_some() {
        ApiError::BadRequest(format!("{error:#}"))
    } else {
        tracing::warn!(%error, "backtest range query failed");
        ApiError::BadGateway("The metrics backend is not answering.".into())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::alerting::model::{AnomalyParams, Operator, RuleKind, Severity, TargetNode};

    fn rule() -> Rule {
        Rule {
            id: 1,
            uid: "disk".to_string(),
            name: "Disk".to_string(),
            description: String::new(),
            kind: RuleKind::Threshold,
            query: "disk".to_string(),
            operator: Operator::Gt,
            threshold: 90.0,
            clear_threshold: None,
            for_duration: Duration::ZERO,
            severity: Severity::Warning,
            selector: model::TargetSelector::All,
            channels: Vec::new(),
            params: AnomalyParams::default(),
            unit: "%".to_string(),
            repeat_interval: None,
            escalate_after: None,
            enabled: true,
            builtin: true,
        }
    }

    fn node(id: TargetId, enabled: bool) -> TargetNode {
        TargetNode {
            id,
            name: format!("nas-{id}"),
            address: format!("10.0.0.{id}"),
            kind: "agent".to_string(),
            parent_id: None,
            via_agent: None,
            interval_secs: 0,
            tags: BTreeMap::new(),
            enabled,
            unreachable: false,
            service_down: false,
        }
    }

    fn range(labels: &[(&str, &str)], points: Vec<(f64, f64)>) -> SeriesRange {
        SeriesRange {
            labels: labels.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect(),
            points,
        }
    }

    #[test]
    fn les_series_sont_filtrees_comme_dans_le_moteur() {
        let grid = Grid { start_s: 0, end_s: 60, step_s: 30 };
        let ranges = vec![
            range(&[("target", "1")], vec![(0.0, 95.0), (30.0, 96.0)]),
            // Même identité après un renommage : fusionnée avec la première.
            range(&[("target", "1"), ("host", "old-name")], vec![(60.0, 97.0)]),
            range(&[("target", "2")], vec![(0.0, 99.0)]),
            range(&[("target", "3")], vec![(0.0, 99.0)]),
            range(&[("target", "404")], vec![(0.0, 99.0)]),
        ];
        let targets = vec![node(1, true), node(2, false), node(3, true)];
        // La règle est retirée pour l'équipement 3, et son seuil relevé pour le 1.
        let overrides = vec![
            RuleOverride {
                rule_uid: "disk".to_string(),
                target_id: 3,
                threshold: None,
                clear_threshold: None,
                enabled: Some(false),
            },
            RuleOverride {
                rule_uid: "disk".to_string(),
                target_id: 1,
                threshold: Some(95.5),
                clear_threshold: None,
                enabled: None,
            },
        ];
        let kept = candidates(&rule(), &ranges, &targets, &overrides, grid);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].target_name, "nas-1");
        assert_eq!(kept[0].values, vec![Some(95.0), Some(96.0), Some(97.0)]);
        assert_eq!(kept[0].effective.threshold, 95.5);
        assert_eq!(kept[0].violations, 2);
    }

    #[test]
    fn les_series_qui_declenchent_viennent_en_tete() {
        let grid = Grid { start_s: 0, end_s: 90, step_s: 30 };
        let ranges = vec![
            range(&[("target", "1")], vec![(0.0, 10.0), (30.0, 10.0)]),
            range(&[("target", "2")], vec![(0.0, 95.0), (30.0, 10.0), (60.0, 95.0)]),
        ];
        let targets = vec![node(1, true), node(2, true)];
        let rule = rule();
        let replayed = replay_all(&rule, candidates(&rule, &ranges, &targets, &[], grid), grid);
        assert_eq!(replayed[0].target_name, "nas-2");
        assert_eq!(replayed[0].periods.len(), 2);
        assert!(replayed[1].periods.is_empty());
        assert_eq!(replayed[0].spark.len(), grid.steps());
    }
}
