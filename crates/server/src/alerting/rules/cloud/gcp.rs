//! Règles livrées de gcp (Google Cloud).

use std::time::Duration;

use super::super::base;
use crate::alerting::model::{Operator, Rule, RuleKind, Severity};

#[cfg(test)]
pub(super) const PRODUCED: &[&str] =
    &["dumbmonit_gcp_instances_failed", "dumbmonit_gcp_sql_instances_failed"];

pub(super) fn rules() -> Vec<Rule> {
    vec![
        Rule {
            description: "Google reports a Compute Engine instance as repairing for 10 minutes (instances stopped by their owner are not counted).".to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(600),
            severity: Severity::Critical,
            unit: "".to_string(),
            repeat_interval: Some(Duration::from_secs(21600)),
            ..base("gcp_instance_failed", "Google Cloud instance failed", RuleKind::Threshold, "dumbmonit_gcp_instances_failed")
        },
        Rule {
            description: "A Cloud SQL instance is in the FAILED or unknown state for 10 minutes.".to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(600),
            severity: Severity::Critical,
            unit: "".to_string(),
            repeat_interval: Some(Duration::from_secs(21600)),
            ..base("gcp_sql_failed", "Google Cloud SQL instance failed", RuleKind::Threshold, "dumbmonit_gcp_sql_instances_failed")
        },
    ]
}
