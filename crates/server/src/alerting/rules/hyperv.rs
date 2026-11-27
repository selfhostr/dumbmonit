//! Règles livrées de Hyper-V (`collectors/winrm/hyperv.rs`).
//!
//! Une machine éteinte n'est pas une panne (un administrateur l'a peut-être
//! arrêtée) : aucune règle ne sonne sur `hyperv_vm_running`. Les règles visent ce
//! qui se dégrade : santé, services d'intégration, réplication, points de
//! contrôle oubliés, saturation de l'hôte.

use std::time::Duration;

use super::base;
use crate::alerting::model::{Operator, Rule, RuleKind, Severity};

/// Les métriques (préfixe `dumbmonit_` compris) que ces règles visent.
#[cfg(test)]
pub(super) const PRODUCED: &[&str] = &[
    "dumbmonit_hyperv_vm_health_ok",
    "dumbmonit_hyperv_vm_integration_ok",
    "dumbmonit_hyperv_vm_replication_ok",
    "dumbmonit_hyperv_vm_checkpoint_oldest_age_seconds",
    "dumbmonit_hyperv_host_cpu_percent",
    "dumbmonit_hyperv_host_memory_used_percent",
];

pub(super) fn rules() -> Vec<Rule> {
    vec![
        Rule {
            description: "The health of a virtual machine is not OK (Hyper-V reports a failure \
                          of the VM, for example its storage)."
                .to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "hyperv_vm_unhealthy",
                "Hyper-V VM unhealthy",
                RuleKind::Threshold,
                "dumbmonit_hyperv_vm_health_ok",
            )
        },
        Rule {
            description: "A running virtual machine no longer answers its heartbeat: the guest \
                          is frozen, or its integration services are stopped."
                .to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "hyperv_vm_integration_lost",
                "Hyper-V VM integration services not answering",
                RuleKind::Threshold,
                "dumbmonit_hyperv_vm_integration_ok",
            )
        },
        Rule {
            description: "Hyper-V Replica reports a warning or a critical state for a virtual \
                          machine: its copy on the replica server is behind or broken."
                .to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "hyperv_vm_replication_unhealthy",
                "Hyper-V replication unhealthy",
                RuleKind::Threshold,
                "dumbmonit_hyperv_vm_replication_ok",
            )
        },
        // Le disque différentiel d'un point de contrôle grossit à chaque écriture
        // et finit par remplir le volume. Ceux d'une sauvegarde ne vivent que le
        // temps du travail et ne sonnent pas.
        Rule {
            description: "A virtual machine has kept a checkpoint for more than 3 days. Its \
                          differencing disk grows with every write and slows the VM down: delete \
                          the checkpoint once it is no longer needed."
                .to_string(),
            operator: Operator::Gt,
            threshold: 3.0 * 24.0 * 3600.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "hyperv_vm_checkpoint_old",
                "Hyper-V VM checkpoint too old",
                RuleKind::Threshold,
                "dumbmonit_hyperv_vm_checkpoint_oldest_age_seconds",
            )
        },
        Rule {
            description: "More than 90% of the memory of the Hyper-V host is in use for 15 \
                          minutes: no room left to start or grow a virtual machine."
                .to_string(),
            operator: Operator::Gt,
            threshold: 90.0,
            clear_threshold: Some(85.0),
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Warning,
            unit: "%".to_string(),
            ..base(
                "hyperv_host_memory_high",
                "Hyper-V host memory almost full",
                RuleKind::Threshold,
                "dumbmonit_hyperv_host_memory_used_percent",
            )
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_identifiants_sont_uniques_et_prefixes() {
        let rules = rules();
        let mut uids: Vec<&str> = rules.iter().map(|r| r.uid.as_str()).collect();
        assert!(uids.iter().all(|uid| uid.starts_with("hyperv_")), "{uids:?}");
        uids.sort_unstable();
        let total = uids.len();
        uids.dedup();
        assert_eq!(uids.len(), total, "two Hyper-V rules share a uid");
    }

    #[test]
    fn chaque_metrique_visee_est_declaree_produite() {
        for rule in rules() {
            assert!(PRODUCED.contains(&rule.query.as_str()), "{} cites {}", rule.uid, rule.query);
        }
    }
}
