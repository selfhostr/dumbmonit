//! Règles livrées de ipmi (contrôleurs de gestion, IPMI v2.0).

use std::time::Duration;

use super::super::base;
use crate::alerting::model::{Operator, Rule, RuleKind, Severity};

#[cfg(test)]
pub(super) const PRODUCED: &[&str] =
    &["dumbmonit_ipmi_psus_faulty", "dumbmonit_ipmi_sensors_out_of_limits"];

pub(super) fn rules() -> Vec<Rule> {
    vec![
        Rule {
            description: "A power supply reports a fault, a lost input or a predicted failure for 5 minutes.".to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(300),
            severity: Severity::Critical,
            unit: "".to_string(),
            repeat_interval: Some(Duration::from_secs(21600)),
            ..base("ipmi_psu_fault", "IPMI power supply fault", RuleKind::Threshold, "dumbmonit_ipmi_psus_faulty")
        },
        Rule {
            description: "A BMC sensor (temperature, fan, voltage) is past its critical threshold for 10 minutes.".to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(600),
            severity: Severity::Warning,
            unit: "".to_string(),
            repeat_interval: Some(Duration::from_secs(21600)),
            ..base("ipmi_sensor_critical", "IPMI sensor out of limits", RuleKind::Threshold, "dumbmonit_ipmi_sensors_out_of_limits")
        },
    ]
}
