//! Règles livrées d'Unraid qui complètent celles de `rules.rs` (grappe arrêtée,
//! disque désactivé, erreurs de lecture, parité, cache plein, conteneur
//! arrêté) : VM plantée, alerte d'Unraid non lue, grappe presque pleine,
//! mémoire épuisée, licence en erreur, essai ou droit aux mises à jour qui
//! arrive à son terme.

use std::time::Duration;

use super::base;
use crate::alerting::model::{Operator, Rule, RuleKind, Severity};

#[cfg(test)]
pub(super) const PRODUCED: &[&str] = &[
    "dumbmonit_unraid_vm_crashed",
    "dumbmonit_unraid_notifications_unread",
    "dumbmonit_unraid_array_used_percent",
    "dumbmonit_unraid_memory_used_percent",
    "dumbmonit_unraid_registration_ok",
    "dumbmonit_unraid_trial_expiry_seconds",
    "dumbmonit_unraid_os_updates_expiry_seconds",
];

pub(super) fn rules() -> Vec<Rule> {
    vec![
        // `CRASHED` est l'état que libvirt donne à une VM dont l'invité s'est
        // effondré : une VM éteinte exprès est `SHUTOFF` et ne sonne pas.
        Rule {
            description: "An Unraid VM is in the crashed state: its guest stopped on its own."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "unraid_vm_crashed",
                "Unraid VM crashed",
                RuleKind::Threshold,
                "dumbmonit_unraid_vm_crashed",
            )
        },
        // Unraid signale par ses notifications ce que l'API ne détaille pas :
        // attribut SMART qui se dégrade, disque trop chaud selon ses propres
        // seuils, mise à jour de greffon en échec. Seul le niveau « alert »
        // sonne ; l'alerte retombe quand la notification est archivée.
        Rule {
            description: "Unraid has raised an alert that nobody has read yet (failing SMART \
                          attribute, disk in error, array problem). Read and archive it in \
                          Unraid to clear this alert."
                .to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "unraid_alert_unread",
                "Unraid alert unread",
                RuleKind::Threshold,
                "dumbmonit_unraid_notifications_unread{importance=\"alert\"}",
            )
        },
        // Un serveur multimédia vit couramment au-delà de 90 % : le seuil est
        // placé là où il ne reste vraiment plus de marge.
        Rule {
            description: "The Unraid array is more than 95% full: new files will soon have \
                          nowhere to go."
                .to_string(),
            operator: Operator::Gt,
            threshold: 95.0,
            clear_threshold: Some(93.0),
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "%".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "unraid_array_almost_full",
                "Unraid array nearly full",
                RuleKind::Threshold,
                "dumbmonit_unraid_array_used_percent",
            )
        },
        // Compté d'après la mémoire disponible : le cache disque n'en est pas.
        Rule {
            description: "The Unraid server has used more than 95% of its memory for fifteen \
                          minutes: the kernel may kill a container or a VM."
                .to_string(),
            operator: Operator::Gt,
            threshold: 95.0,
            clear_threshold: Some(90.0),
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Warning,
            unit: "%".to_string(),
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "unraid_memory_high",
                "Unraid memory nearly exhausted",
                RuleKind::Threshold,
                "dumbmonit_unraid_memory_used_percent",
            )
        },
        // Clé absente, clé USB remplacée, essai expiré, clé révoquée : la grappe
        // tourne encore mais ne redémarrera pas. Trente minutes laissent passer
        // un état transitoire au démarrage.
        Rule {
            description: "Unraid reports a problem with its license key (missing key file, \
                          changed flash drive, expired trial, blacklisted key): the array will \
                          not start at the next boot."
                .to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(30 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "unraid_license_problem",
                "Unraid license problem",
                RuleKind::Threshold,
                "dumbmonit_unraid_registration_ok",
            )
        },
        Rule {
            description: "The Unraid trial ends in less than three days: without a license \
                          key the array will not start after that."
                .to_string(),
            operator: Operator::Lt,
            threshold: 3.0 * 86_400.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "unraid_trial_expiring",
                "Unraid trial ending",
                RuleKind::Threshold,
                "dumbmonit_unraid_trial_expiry_seconds",
            )
        },
        // Le serveur continue de fonctionner après l'échéance, il ne reçoit
        // simplement plus les nouvelles versions : simple information, et le
        // filtre `> 0` la fait taire une fois la date passée.
        Rule {
            description: "The license of this Unraid server stops covering new OS versions in \
                          less than thirty days. The server keeps working; extend the license \
                          to keep receiving updates."
                .to_string(),
            operator: Operator::Lt,
            threshold: 30.0 * 86_400.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Info,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(7 * 24 * 3600)),
            ..base(
                "unraid_os_updates_ending",
                "Unraid OS updates eligibility ending",
                RuleKind::Threshold,
                "dumbmonit_unraid_os_updates_expiry_seconds > 0",
            )
        },
    ]
}
