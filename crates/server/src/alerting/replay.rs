//! Rejeu d'une règle sur l'historique : « tester sur l'historique ».
//!
//! On rejoue la règle telle qu'elle est en cours d'édition sur les mesures
//! déjà stockées, au pas de l'évaluation du moteur, et on dit ce qu'elle
//! aurait fait : quand elle aurait déclenché, pour combien de temps, jusqu'à
//! quelle valeur, et combien de notifications cela aurait représenté.
//!
//! Rien n'est réécrit ici : chaque pas passe par [`evaluation::threshold_step`]
//! — la même fonction que le moteur — et chaque notification par
//! [`group::decide`]. Les surcouches qui n'existent pas dans le passé rejoué
//! (suppression par dépendance, maintenance, acquittement, politique de
//! notification des canaux) ne sont pas simulées : le décompte est celui de la
//! règle seule, avant ces filtres.
//!
//! Seules les règles dont la condition est un seuil sur une requête MetricsQL
//! se rejouent ([`not_replayable`]) : les autres dépendent d'un état que le
//! passé ne contient pas (baselines apprises, journaux, calculs dérivés).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::alerting::evaluation;
use crate::alerting::group::{self, AlertOutcome, NotifyReason};
use crate::alerting::machine::{AlertState, Phase, Transition};
use crate::alerting::model::{Operator, Rule, RuleKind};
use crate::alerting::overrides::Effective;

/// Nombre de points de la miniature renvoyée par série.
pub const SPARK_POINTS: usize = 96;

/// Une règle qui ne se rejoue pas, et pourquoi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NotReplayable {
    /// Code stable, que l'interface traduit.
    pub code: &'static str,
    /// La même chose en une phrase, pour les clients de l'API.
    pub reason: &'static str,
}

/// `None` quand la règle se rejoue ; sinon la raison.
pub fn not_replayable(kind: RuleKind) -> Option<NotReplayable> {
    let (code, reason) = match kind {
        RuleKind::Threshold | RuleKind::Predict => return None,
        RuleKind::Anomaly => (
            "anomaly",
            "An anomaly rule compares each value to a baseline learned week after week: the \
             past cannot be replayed without relearning it.",
        ),
        RuleKind::LogPattern => (
            "log_pattern",
            "A log pattern rule counts log lines, not stored metrics: it cannot be replayed.",
        ),
        RuleKind::Forecast => (
            "forecast",
            "A forecast rule extrapolates days of history at each evaluation: replaying it would \
             recompute a forecast per step.",
        ),
        RuleKind::Drift => (
            "drift",
            "A drift rule compares each value to past weeks computed at evaluation time: it \
             cannot be replayed.",
        ),
        RuleKind::SloBurn => (
            "slo_burn",
            "An error budget rule is computed from the SLOs at evaluation time, not from a \
             stored query: it cannot be replayed.",
        ),
    };
    Some(NotReplayable { code, reason })
}

/// Grille de temps du rejeu, en secondes Unix : `start_s`, `start_s + step_s`…
/// jusqu'à `end_s` compris.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub start_s: i64,
    pub end_s: i64,
    pub step_s: i64,
}

impl Grid {
    /// Grille de `span_s` secondes qui finit à `end_s`, au pas de
    /// l'évaluation `interval_s`. Au-delà de `max_steps` pas, le pas est élargi
    /// (au multiple de l'intervalle supérieur) pour borner le coût : c'est
    /// alors une approximation, que l'appelant doit signaler.
    pub fn new(end_s: i64, span_s: i64, interval_s: i64, max_steps: u64) -> Self {
        let interval = interval_s.max(1).unsigned_abs();
        let span = span_s.max(0).unsigned_abs();
        let max_steps = max_steps.max(1);
        let mut step = interval;
        if span / step > max_steps {
            step = span.div_ceil(max_steps).div_ceil(interval) * interval;
        }
        let step = i64::try_from(step).unwrap_or(i64::MAX);
        let end = end_s - end_s.rem_euclid(step);
        let span = i64::try_from(span).unwrap_or(i64::MAX);
        Self { start_s: end - (span / step) * step, end_s: end, step_s: step }
    }

    /// Nombre de pas, bornes comprises.
    pub fn steps(&self) -> usize {
        usize::try_from((self.end_s - self.start_s) / self.step_s + 1).unwrap_or(0)
    }

    /// Instant du pas `index`.
    pub fn at(&self, index: usize) -> i64 {
        self.start_s + i64::try_from(index).unwrap_or(i64::MAX) * self.step_s
    }

    /// Pas le plus proche de l'horodatage `t` (secondes), s'il tombe dans la grille.
    pub fn index_of(&self, t: f64) -> Option<usize> {
        let offset = ((t - self.start_s as f64) / self.step_s as f64).round();
        if !offset.is_finite() || offset < 0.0 {
            return None;
        }
        let index = offset as usize;
        (index < self.steps()).then_some(index)
    }

    /// Range des points `(secondes, valeur)` sur la grille : `None` là où la
    /// série n'a rien. Un second point sur le même pas ne remplace pas le premier.
    pub fn align(&self, points: &[(f64, f64)], values: &mut [Option<f64>]) {
        for &(t, value) in points {
            if let Some(slot) = self.index_of(t).and_then(|index| values.get_mut(index))
                && slot.is_none()
            {
                *slot = Some(value);
            }
        }
    }
}

/// Une période pendant laquelle l'alerte aurait été `firing`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FiringPeriod {
    /// Début, en secondes Unix.
    pub start: i64,
    /// Fin, en secondes Unix ; `None` si elle durait encore à la fin de la fenêtre.
    pub end: Option<i64>,
    /// Valeur la plus défavorable atteinte pendant la période (la plus haute
    /// pour « > », la plus basse pour « < »).
    pub peak: f64,
}

/// Ce qu'un pas du rejeu a produit.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub phase: Phase,
    pub transition: Option<Transition>,
    pub notified: Option<NotifyReason>,
}

/// Bilan du rejeu d'une série.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Replayed {
    pub periods: Vec<FiringPeriod>,
    /// Notifications qu'aurait émises la règle : déclenchements, rappels,
    /// escalades et résolutions, avant maintenance et politique des canaux.
    pub notifications: u32,
    /// Dépassements retombés avant la fin de `for` : ce que la durée de
    /// confirmation a évité.
    pub absorbed: u32,
    /// Durée totale passée en `firing`, en secondes.
    pub firing_secs: i64,
}

/// Rejoue une série pas à pas, avec la machine du moteur.
pub struct Replayer<'a> {
    rule: &'a Rule,
    effective: Effective,
    state: AlertState,
    periods: Vec<FiringPeriod>,
    notifications: u32,
    absorbed: u32,
}

impl<'a> Replayer<'a> {
    pub fn new(rule: &'a Rule, effective: Effective) -> Self {
        Self {
            rule,
            effective,
            state: AlertState::default(),
            periods: Vec::new(),
            notifications: 0,
            absorbed: 0,
        }
    }

    /// Avance d'un pas. `value` à `None` : aucune mesure à cet instant.
    pub fn push(&mut self, now: DateTime<Utc>, value: Option<f64>) -> Step {
        let (mut state, transition) =
            evaluation::threshold_step(&self.state, self.rule, &self.effective, value, now);

        match &transition {
            Some(t) if t.to == Phase::Firing => self.periods.push(FiringPeriod {
                start: now.timestamp(),
                end: None,
                peak: value.unwrap_or_default(),
            }),
            Some(t) if t.from == Phase::Firing => {
                if let Some(period) = self.periods.last_mut() {
                    period.end = Some(now.timestamp());
                }
            }
            Some(t) if t.from == Phase::Pending && t.to == Phase::Ok => self.absorbed += 1,
            _ => {}
        }
        if state.phase == Phase::Firing
            && let (Some(value), Some(period)) = (value, self.periods.last_mut())
        {
            period.peak = worse(self.rule.operator, period.peak, value);
        }

        let notified = would_notify(self.rule, &state, now);
        if notified.is_some() {
            evaluation::mark_notified(&mut state, now);
            self.notifications += 1;
        }
        self.state = state;
        Step { phase: self.state.phase, transition, notified }
    }

    /// Clôt le rejeu à `end_s` : une période encore ouverte compte jusque-là.
    pub fn finish(self, end_s: i64) -> Replayed {
        let firing_secs = self
            .periods
            .iter()
            .map(|period| (period.end.unwrap_or(end_s) - period.start).max(0))
            .sum();
        Replayed {
            periods: self.periods,
            notifications: self.notifications,
            absorbed: self.absorbed,
            firing_secs,
        }
    }
}

/// Rejoue une série déjà rangée sur la grille.
pub fn replay_series(
    rule: &Rule,
    effective: Effective,
    values: &[Option<f64>],
    grid: Grid,
) -> Replayed {
    let mut replayer = Replayer::new(rule, effective);
    for (index, value) in values.iter().enumerate() {
        let Some(now) = DateTime::from_timestamp(grid.at(index), 0) else { continue };
        replayer.push(now, *value);
    }
    replayer.finish(grid.end_s)
}

/// Résume une série en au plus `points` valeurs pour une miniature : la plus
/// défavorable de chaque tranche, les trous comblés par la valeur précédente
/// (ou la première connue). Vide si la série n'a aucune valeur.
pub fn spark(values: &[Option<f64>], operator: Operator, points: usize) -> Vec<f64> {
    if values.is_empty() || points == 0 {
        return Vec::new();
    }
    let chunk = values.len().div_ceil(points);
    let buckets: Vec<Option<f64>> = values
        .chunks(chunk)
        .map(|slice| slice.iter().flatten().copied().reduce(|a, b| worse(operator, a, b)))
        .collect();
    let Some(first) = buckets.iter().flatten().copied().next() else { return Vec::new() };
    let mut last = first;
    buckets
        .into_iter()
        .map(|bucket| {
            if let Some(value) = bucket {
                last = value;
            }
            last
        })
        .collect()
}

/// La plus défavorable de deux valeurs au sens de l'opérateur.
fn worse(operator: Operator, a: f64, b: f64) -> f64 {
    match operator {
        Operator::Gt | Operator::Ge => a.max(b),
        Operator::Lt | Operator::Le => a.min(b),
    }
}

/// Ce que [`group::decide`] dirait de cet état : la même décision que le moteur.
fn would_notify(rule: &Rule, state: &AlertState, now: DateTime<Utc>) -> Option<NotifyReason> {
    let outcome = AlertOutcome {
        fingerprint: String::new(),
        rule_uid: String::new(),
        rule_name: String::new(),
        target_id: None,
        target_name: String::new(),
        series_key: String::new(),
        labels: BTreeMap::new(),
        state: state.clone(),
        severity: rule.severity,
        value: state.value,
        score: None,
        unit: String::new(),
        operator: String::new(),
        threshold: 0.0,
        channels: Vec::new(),
        repeat_interval: rule.repeat_interval,
        escalate_after: rule.escalate_after,
        // Le relais d'escalade dépend de la politique de l'instance, pas de la
        // règle : il n'est pas compté.
        unacked_after: None,
        just_transitioned: false,
        recurred: false,
        detail: None,
    };
    group::decide(&outcome, now)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::*;
    use crate::alerting::cycle::{
        BaselineStore, CycleInput, CycleOutcome, RuleObservations, StoredAlert, plan_cycle,
    };
    use crate::alerting::model::{AnomalyParams, Severity, TargetNode, TargetSelector};
    use crate::alerting::notify_policy::ResolutionsDuringSilence;
    use crate::alerting::source::SeriesPoint;

    const BASE: i64 = 1_700_000_000;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(BASE + seconds, 0).expect("valid timestamp")
    }

    fn rule(for_secs: u64, repeat_secs: Option<u64>, escalate_secs: Option<u64>) -> Rule {
        Rule {
            id: 1,
            uid: "cpu_high".to_string(),
            name: "CPU high".to_string(),
            description: String::new(),
            kind: RuleKind::Threshold,
            query: "cpu".to_string(),
            operator: Operator::Gt,
            threshold: 90.0,
            clear_threshold: Some(80.0),
            for_duration: Duration::from_secs(for_secs),
            severity: Severity::Warning,
            selector: TargetSelector::All,
            channels: Vec::new(),
            params: AnomalyParams::default(),
            unit: "%".to_string(),
            repeat_interval: repeat_secs.map(Duration::from_secs),
            escalate_after: escalate_secs.map(Duration::from_secs),
            enabled: true,
            builtin: false,
        }
    }

    fn effective(rule: &Rule) -> Effective {
        Effective {
            threshold: rule.threshold,
            clear_threshold: rule.clear_threshold,
            enabled: true,
        }
    }

    fn node() -> TargetNode {
        TargetNode {
            id: 1,
            name: "nas".to_string(),
            address: "10.0.0.1".to_string(),
            kind: "agent".to_string(),
            parent_id: None,
            via_agent: None,
            interval_secs: 0,
            tags: BTreeMap::new(),
            enabled: true,
            unreachable: false,
            service_down: false,
        }
    }

    fn point(value: f64, now: DateTime<Utc>) -> SeriesPoint {
        SeriesPoint {
            labels: [("__name__", "cpu"), ("target", "1")]
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            value,
            ts_ms: now.timestamp_millis(),
        }
    }

    /// L'état que le moteur relit au cycle suivant, notifications prises en
    /// charge comprises — exactement ce que font `save_states` et
    /// `mark_notified`.
    fn carry(outcome: &CycleOutcome, now: DateTime<Utc>) -> Vec<StoredAlert> {
        let notified: Vec<&str> = outcome
            .groups
            .iter()
            .flat_map(|group| group.items.iter().map(|item| item.fingerprint.as_str()))
            .collect();
        outcome
            .alerts
            .iter()
            .map(|alert| {
                let mut state = alert.state.clone();
                if notified.contains(&alert.fingerprint.as_str()) {
                    evaluation::mark_notified(&mut state, now);
                }
                StoredAlert {
                    fingerprint: alert.fingerprint.clone(),
                    rule_uid: alert.rule_uid.clone(),
                    target_id: alert.target_id,
                    series_key: alert.series_key.clone(),
                    labels: alert.labels.clone(),
                    state,
                }
            })
            .collect()
    }

    /// Une suite de mesures écrite en clair : `-` pour un pas sans mesure.
    fn readings(text: &str) -> Vec<Option<f64>> {
        text.split_whitespace().map(|word| word.parse().ok()).collect()
    }

    /// Fait tourner le moteur et le rejeu sur la même suite de valeurs et
    /// exige, à chaque pas, la même phase, la même transition et la même
    /// décision de notifier.
    fn assert_same_as_engine(rule: &Rule, values: &[Option<f64>]) -> Replayed {
        let mut stored: Vec<StoredAlert> = Vec::new();
        let mut baselines = BaselineStore::default();
        let mut replayer = Replayer::new(rule, effective(rule));
        for (index, value) in values.iter().enumerate() {
            let now = at(index as i64 * 30);
            let series = value.map(|v| vec![point(v, now)]).unwrap_or_default();
            let input = CycleInput {
                now,
                observations: vec![RuleObservations { rule: rule.clone(), series: Some(series) }],
                targets: vec![node()],
                previous: stored.clone(),
                silences: Vec::new(),
                overrides: Vec::new(),
                unacked_after: None,
                resolutions_during_silence: ResolutionsDuringSilence::default(),
                occurrences: HashMap::new(),
                annotations: HashMap::new(),
            };
            let outcome = plan_cycle(input, &mut baselines);
            let live_phase = outcome.alerts.first().map_or(Phase::Ok, |alert| alert.state.phase);
            let live_transition = outcome.history.first().map(|entry| entry.transition.clone());
            let live_reason = outcome.groups.first().map(|group| group.items[0].reason);

            let step = replayer.push(now, *value);
            assert_eq!(step.phase, live_phase, "phase at step {index}");
            assert_eq!(step.transition, live_transition, "transition at step {index}");
            assert_eq!(step.notified, live_reason, "notification at step {index}");
            stored = carry(&outcome, now);
        }
        replayer.finish(at(values.len() as i64 * 30).timestamp())
    }

    #[test]
    fn le_rejeu_reproduit_le_moteur_pas_a_pas() {
        // Monte, tient dans l'hystérésis, disparaît (trou de collecte), revient,
        // oscille autour du seuil, puis reste haut assez longtemps pour les
        // rappels et l'escalade.
        let mut values = readings("50 95 96 97 85 84 - 99 92 70 91 60 91 91.5");
        values.extend(std::iter::repeat_n(Some(98.0), 20));
        values.extend([Some(10.0), Some(10.0), Some(10.0)]);

        for rule in [
            rule(0, None, None),
            rule(60, None, None),
            rule(60, Some(120), None),
            rule(30, Some(90), Some(300)),
        ] {
            let replayed = assert_same_as_engine(&rule, &values);
            assert!(!replayed.periods.is_empty());
        }
    }

    #[test]
    fn les_periodes_portent_leurs_bornes_et_leur_pic() {
        let rule = rule(0, None, None);
        let values = [Some(50.0), Some(95.0), Some(99.0), Some(85.0), Some(70.0), Some(93.0)];
        let replayed = assert_same_as_engine(&rule, &values);
        assert_eq!(
            replayed.periods,
            vec![
                FiringPeriod { start: BASE + 30, end: Some(BASE + 120), peak: 99.0 },
                FiringPeriod { start: BASE + 150, end: None, peak: 93.0 },
            ]
        );
        // Ouverte jusqu'à la fin de la fenêtre (six pas, soit 180 s).
        assert_eq!(replayed.firing_secs, 90 + 30);
        // Déclenchement, résolution, nouveau déclenchement.
        assert_eq!(replayed.notifications, 3);
    }

    #[test]
    fn le_for_absorbe_les_pointes_breves() {
        let rule = rule(90, None, None);
        let values = [Some(95.0), Some(95.0), Some(10.0), Some(95.0), Some(10.0)];
        let replayed = assert_same_as_engine(&rule, &values);
        assert!(replayed.periods.is_empty());
        assert_eq!(replayed.absorbed, 2);
        assert_eq!(replayed.notifications, 0);
    }

    #[test]
    fn les_rappels_sont_comptes() {
        // Rappel toutes les 60 s, alerte tenue 240 s : un déclenchement, quatre
        // rappels, puis la résolution.
        let rule = rule(0, Some(60), None);
        let mut values = vec![Some(95.0); 9];
        values.push(Some(10.0));
        let replayed = assert_same_as_engine(&rule, &values);
        assert_eq!(replayed.periods.len(), 1);
        assert_eq!(replayed.notifications, 1 + 4 + 1);
    }

    #[test]
    fn la_grille_suit_l_intervalle_et_s_elargit_au_dela_du_plafond() {
        let grid = Grid::new(BASE + 17, 86_400, 30, 10_000);
        assert_eq!(grid.step_s, 30);
        assert_eq!(grid.end_s % 30, 0);
        assert_eq!(grid.end_s - grid.start_s, 86_400);
        assert_eq!(grid.steps(), 2_881);

        // Trente jours au pas de 30 s dépassent le plafond : pas élargi, multiple de 30.
        let wide = Grid::new(BASE, 30 * 86_400, 30, 10_000);
        assert_eq!(wide.step_s, 270);
        assert!(wide.steps() <= 10_001);
        assert_eq!(wide.start_s % 270, 0);
    }

    #[test]
    fn les_points_sont_ranges_sur_le_pas_le_plus_proche() {
        let grid = Grid { start_s: 1_000, end_s: 1_120, step_s: 30 };
        let mut values = vec![None; grid.steps()];
        grid.align(&[(1_000.0, 1.0), (1_031.0, 2.0), (1_032.0, 9.0), (2_000.0, 3.0)], &mut values);
        assert_eq!(values, vec![Some(1.0), Some(2.0), None, None, None]);
        assert_eq!(grid.index_of(999.0), Some(0));
        assert_eq!(grid.index_of(900.0), None);
    }

    #[test]
    fn la_miniature_garde_le_pire_de_chaque_tranche_et_comble_les_trous() {
        let values = [None, Some(1.0), Some(5.0), None, None, None, Some(2.0), Some(3.0)];
        assert_eq!(spark(&values, Operator::Gt, 4), vec![1.0, 5.0, 5.0, 3.0]);
        assert_eq!(spark(&values, Operator::Lt, 4), vec![1.0, 5.0, 5.0, 2.0]);
        assert!(spark(&[None, None], Operator::Gt, 4).is_empty());
    }

    #[test]
    fn seules_les_regles_a_seuil_se_rejouent() {
        assert!(not_replayable(RuleKind::Threshold).is_none());
        assert!(not_replayable(RuleKind::Predict).is_none());
        for kind in [
            RuleKind::Anomaly,
            RuleKind::LogPattern,
            RuleKind::Forecast,
            RuleKind::Drift,
            RuleKind::SloBurn,
        ] {
            let why = not_replayable(kind).expect("not replayable");
            assert_eq!(why.code, kind.as_str());
        }
    }
}
