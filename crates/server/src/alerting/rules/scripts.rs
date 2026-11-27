//! Règle livrée des contrôles personnalisés (`collectors/agent/scripts.rs`).
//!
//! Un CRITICAL met déjà le moniteur hors service (alerte « injoignable », avec
//! le message du contrôle) ; reste l'avertissement, qu'un greffon Nagios rend
//! par le code 1 : la série `script_warning` vaut alors 1.

use std::time::Duration;

use super::base;
use crate::alerting::model::{Operator, Rule, RuleKind, Severity};

pub(super) fn rules() -> Vec<Rule> {
    vec![Rule {
        description: "The custom check returned WARNING (exit code 1 of a Nagios plugin, or \"status\": \"warning\")."
            .to_string(),
        operator: Operator::Gt,
        threshold: 0.0,
        // Deux exécutions au moins à la période par défaut : un seul
        // avertissement isolé ne réveille personne.
        for_duration: Duration::from_secs(2 * 60),
        severity: Severity::Warning,
        repeat_interval: Some(Duration::from_secs(6 * 3600)),
        ..base(
            "script_warning",
            "Custom check warning",
            RuleKind::Threshold,
            "dumbmonit_script_warning",
        )
    }]
}
