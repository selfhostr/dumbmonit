//! Règles livrées de pfSense qui complètent celles de `rules.rs` (passerelle
//! tombée, perte, latence, interface sans lien, service arrêté, disque plein) :
//! table d'états, mbufs, température, processeur, swap, erreurs d'interface et
//! nœud CARP laissé en maintenance. Les seuils sont ceux d'OPNsense, pour que
//! les deux pare-feu sonnent pareil.
//!
//! Pas de règle sur la mémoire : pfSense compte le cache ZFS dans la mémoire
//! utilisée, et une installation ZFS saine afficherait 90 % en permanence. Le
//! swap dit mieux qu'un pare-feu manque de mémoire.

use std::time::Duration;

use super::base;
use crate::alerting::model::{Operator, Rule, RuleKind, Severity};

#[cfg(test)]
pub(super) const PRODUCED: &[&str] = &[
    "dumbmonit_pfsense_states_used_percent",
    "dumbmonit_pfsense_mbuf_used_percent",
    "dumbmonit_pfsense_temperature_celsius",
    "dumbmonit_pfsense_cpu_usage_percent",
    "dumbmonit_pfsense_swap_used_percent",
    "dumbmonit_pfsense_interface_errors_in_total",
    "dumbmonit_pfsense_interface_errors_out_total",
    "dumbmonit_pfsense_carp_maintenance_mode",
];

pub(super) fn rules() -> Vec<Rule> {
    vec![
        // Table pleine, le pare-feu refuse toute nouvelle connexion alors que
        // les passerelles et les services ont l'air sains : la panne la plus
        // difficile à diagnostiquer après coup.
        Rule {
            description: "The firewall state table is more than 80 % full: once it is full, new connections are refused. Look for a flood or a scan, or raise the maximum under System > Advanced > Firewall & NAT.".to_string(),
            operator: Operator::Gt,
            threshold: 80.0,
            clear_threshold: Some(70.0),
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            unit: "%".to_string(),
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "pfsense_states_table_full",
                "pfSense state table nearly full",
                RuleKind::Threshold,
                "dumbmonit_pfsense_states_used_percent",
            )
        },
        Rule {
            description: "More than 90 % of the network memory buffers (mbufs) are in use: when none are left, the firewall stops passing traffic until it is rebooted.".to_string(),
            operator: Operator::Gt,
            threshold: 90.0,
            clear_threshold: Some(80.0),
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            unit: "%".to_string(),
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "pfsense_mbuf_high",
                "pfSense network buffers nearly exhausted",
                RuleKind::Threshold,
                "dumbmonit_pfsense_mbuf_used_percent",
            )
        },
        // pfSense ne donne une température que si un capteur est déclaré
        // (System > Advanced > Miscellaneous > Thermal sensors) : sans lui, la
        // métrique n'existe pas et la règle ne voit rien.
        Rule {
            description: "The firewall reports more than 85 °C: check the fan, the dust and the room temperature before it throttles or shuts down.".to_string(),
            operator: Operator::Gt,
            threshold: 85.0,
            clear_threshold: Some(78.0),
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            unit: "°C".to_string(),
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "pfsense_temperature_high",
                "pfSense temperature high",
                RuleKind::Threshold,
                "dumbmonit_pfsense_temperature_celsius",
            )
        },
        Rule {
            description: "The firewall's processor has been above 90 % for fifteen minutes: throughput drops and VPN tunnels slow down.".to_string(),
            operator: Operator::Gt,
            threshold: 90.0,
            clear_threshold: Some(80.0),
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Warning,
            unit: "%".to_string(),
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "pfsense_cpu_high",
                "pfSense CPU high",
                RuleKind::Threshold,
                "dumbmonit_pfsense_cpu_usage_percent",
            )
        },
        // Une heure, pour ne pas réagir au pic d'une mise à jour de paquets.
        Rule {
            description: "The firewall uses more than half of its swap for an hour: it is short of memory.".to_string(),
            operator: Operator::Gt,
            threshold: 50.0,
            clear_threshold: Some(40.0),
            for_duration: Duration::from_secs(3600),
            severity: Severity::Warning,
            unit: "%".to_string(),
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "pfsense_swap_high",
                "pfSense swap in use",
                RuleKind::Threshold,
                "dumbmonit_pfsense_swap_used_percent",
            )
        },
        // Cent erreurs en un quart d'heure : un câble, un SFP ou un duplex mal
        // négocié, pas le bruit de fond d'un lien sain.
        Rule {
            description: "An interface logged more than 100 receive or transmit errors in 15 minutes: check the cable, the SFP and the duplex.".to_string(),
            operator: Operator::Gt,
            threshold: 100.0,
            for_duration: Duration::from_secs(15 * 60),
            severity: Severity::Warning,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "pfsense_interface_errors",
                "pfSense interface errors",
                RuleKind::Threshold,
                "increase_prometheus(dumbmonit_pfsense_interface_errors_in_total[15m]) + increase_prometheus(dumbmonit_pfsense_interface_errors_out_total[15m])",
            )
        },
        // Le mode maintenance se met à la main avant une intervention et
        // s'oublie : la paire CARP reste alors sans secours.
        Rule {
            description: "This node has been in CARP maintenance mode for an hour: it will not take over if the other node fails. Leave maintenance mode under Status > CARP once the work is done.".to_string(),
            operator: Operator::Gt,
            threshold: 0.0,
            for_duration: Duration::from_secs(3600),
            severity: Severity::Info,
            repeat_interval: Some(Duration::from_secs(24 * 3600)),
            ..base(
                "pfsense_carp_maintenance",
                "pfSense CARP maintenance mode left on",
                RuleKind::Threshold,
                "dumbmonit_pfsense_carp_maintenance_mode",
            )
        },
    ]
}
