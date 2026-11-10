//! Règles livrées de Veeam Backup & Replication qui complètent celles de
//! `rules.rs` (travail en échec, en avertissement, trop ancien, dépôt plein,
//! licence et support qui expirent) : dépôt hors ligne, licence expirée ou
//! invalide, sauvegarde de configuration désactivée ou trop ancienne.

use std::time::Duration;

use super::base;
use crate::alerting::model::{Operator, Rule, RuleKind, Severity};

#[cfg(test)]
pub(super) const PRODUCED: &[&str] = &[
    "dumbmonit_veeam_repository_online",
    "dumbmonit_veeam_license_valid",
    "dumbmonit_veeam_config_backup_enabled",
    "dumbmonit_veeam_config_backup_last_success_age_seconds",
];

pub(super) fn rules() -> Vec<Rule> {
    vec![
        // Publié seulement quand l'API donne `isOnline` (révision 1.2 et
        // suivantes) : avec l'en-tête par défaut, la règle ne voit rien.
        Rule {
            description: "A Veeam backup repository is offline: every job that writes to it will fail. Check the repository server, its share or its credentials.".to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "veeam_repository_offline",
                "Veeam repository offline",
                RuleKind::Threshold,
                "dumbmonit_veeam_repository_online",
            )
        },
        // `status` vaut `Valid`, `Expired` ou `Invalid`. La règle d'expiration
        // prévient trente jours avant ; celle-ci dit que c'est fait, ou que la
        // licence n'est plus reconnue.
        Rule {
            description: "The Veeam license is expired or invalid: jobs stop running at the end of the grace period.".to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(30 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "veeam_license_invalid",
                "Veeam license expired or invalid",
                RuleKind::Threshold,
                "dumbmonit_veeam_license_valid",
            )
        },
        Rule {
            description: "The Veeam configuration backup is disabled: if the backup server is lost, its jobs, credentials and catalog must be rebuilt by hand.".to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(24 * 3600),
            severity: Severity::Info,
            repeat_interval: Some(Duration::from_secs(7 * 24 * 3600)),
            ..base(
                "veeam_config_backup_disabled",
                "Veeam configuration backup disabled",
                RuleKind::Threshold,
                "dumbmonit_veeam_config_backup_enabled",
            )
        },
        // Veeam la planifie tous les jours par défaut ; sept jours laissent
        // passer une planification hebdomadaire sans sonner.
        Rule {
            description: "The last successful Veeam configuration backup is more than 7 days old although it is enabled: check its repository and its last session.".to_string(),
            operator: Operator::Gt,
            threshold: 7.0 * 24.0 * 3600.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "veeam_config_backup_old",
                "Veeam configuration backup too old",
                RuleKind::Threshold,
                "dumbmonit_veeam_config_backup_last_success_age_seconds and on (target) dumbmonit_veeam_config_backup_enabled == 1",
            )
        },
    ]
}
