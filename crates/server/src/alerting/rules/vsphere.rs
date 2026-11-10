//! Règles livrées de vSphere qui complètent celles de `rules.rs` (hôte qui ne
//! répond plus, hôte rouge, maintenance oubliée, banque de données pleine ou
//! inaccessible, VMware Tools arrêtés, alarme rouge) : instantané oublié et
//! disques à consolider.

use std::time::Duration;

use super::base;
use crate::alerting::model::{Operator, Rule, RuleKind, Severity};

#[cfg(test)]
pub(super) const PRODUCED: &[&str] = &[
    "dumbmonit_vsphere_vm_snapshot_oldest_age_seconds",
    "dumbmonit_vsphere_vm_consolidation_needed",
];

pub(super) fn rules() -> Vec<Rule> {
    vec![
        // VMware recommande de ne pas garder un instantané plus de 72 heures :
        // son fichier delta grossit à chaque écriture, ralentit la VM et finit
        // par remplir la banque de données. Ceux d'une sauvegarde ne vivent que
        // le temps du travail et ne sonnent pas.
        Rule {
            description: "A VM has kept a snapshot for more than 3 days. Its delta file grows with every write, slows the VM down and ends up filling the datastore: delete the snapshot once it is no longer needed.".to_string(),
            operator: Operator::Gt,
            threshold: 3.0 * 24.0 * 3600.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "vsphere_vm_snapshot_old",
                "vSphere VM snapshot too old",
                RuleKind::Threshold,
                "dumbmonit_vsphere_vm_snapshot_oldest_age_seconds",
            )
        },
        // Une heure : pendant une sauvegarde ou une suppression d'instantané,
        // le drapeau se lève quelques minutes sans que rien n'aille mal.
        Rule {
            description: "The disks of a VM need consolidation: a snapshot was removed but its delta files remain, usually after an interrupted backup. Run Snapshots > Consolidate on the VM.".to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "vsphere_vm_consolidation_needed",
                "vSphere VM disks need consolidation",
                RuleKind::Threshold,
                "dumbmonit_vsphere_vm_consolidation_needed",
            )
        },
    ]
}
