//! Règles livrées de MySQL / MariaDB (`collectors/uptime/sql/mysqlstats.rs`).
//!
//! La disponibilité (`probe_success`) reste couverte par « Monitor unreachable » ;
//! ici, la santé interne du serveur. Un serveur sans réplication ne produit aucune
//! série de réplica : ces règles restent muettes.

use std::time::Duration;

use super::base;
use crate::alerting::model::{Operator, Rule, RuleKind, Severity};

/// Les métriques (préfixe `dumbmonit_` compris) que ces règles visent.
#[cfg(test)]
pub(super) const PRODUCED: &[&str] = &[
    "dumbmonit_mysql_connections_used_percent",
    "dumbmonit_mysql_replica_running",
    "dumbmonit_mysql_replica_lag_seconds",
    "dumbmonit_mysql_uptime_seconds",
];

pub(super) fn rules() -> Vec<Rule> {
    vec![
        Rule {
            description: "More than 85% of max_connections is in use: new clients will soon be \
                          refused (Too many connections)."
                .to_string(),
            operator: Operator::Gt,
            threshold: 85.0,
            clear_threshold: Some(80.0),
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            unit: "%".to_string(),
            escalate_after: Some(Duration::from_secs(3600)),
            ..base(
                "mysql_connections_high",
                "MySQL connections near the limit",
                RuleKind::Threshold,
                "dumbmonit_mysql_connections_used_percent",
            )
        },
        Rule {
            description: "A replication channel has its I/O or SQL thread stopped for 5 \
                          minutes: the replica no longer follows its source."
                .to_string(),
            operator: Operator::Lt,
            threshold: 1.0,
            for_duration: Duration::from_secs(5 * 60),
            severity: Severity::Critical,
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "mysql_replica_not_running",
                "MySQL replication stopped",
                RuleKind::Threshold,
                "dumbmonit_mysql_replica_running",
            )
        },
        Rule {
            description: "A replica is more than 5 minutes behind its source.".to_string(),
            operator: Operator::Gt,
            threshold: 300.0,
            clear_threshold: Some(120.0),
            for_duration: Duration::from_secs(10 * 60),
            severity: Severity::Warning,
            unit: "s".to_string(),
            repeat_interval: Some(Duration::from_secs(6 * 3600)),
            ..base(
                "mysql_replica_lagging",
                "MySQL replica lagging",
                RuleKind::Threshold,
                "dumbmonit_mysql_replica_lag_seconds",
            )
        },
        Rule {
            description: "MySQL started less than 5 minutes ago: it restarted, perhaps after a \
                          crash."
                .to_string(),
            operator: Operator::Lt,
            threshold: 300.0,
            for_duration: Duration::from_secs(60),
            severity: Severity::Info,
            unit: "s".to_string(),
            repeat_interval: None,
            ..base(
                "mysql_restarted",
                "MySQL restarted",
                RuleKind::Threshold,
                "dumbmonit_mysql_uptime_seconds",
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
        assert!(uids.iter().all(|uid| uid.starts_with("mysql_")), "{uids:?}");
        uids.sort_unstable();
        let total = uids.len();
        uids.dedup();
        assert_eq!(uids.len(), total, "two MySQL rules share a uid");
    }

    #[test]
    fn chaque_metrique_visee_est_declaree_produite() {
        for rule in rules() {
            assert!(PRODUCED.contains(&rule.query.as_str()), "{} cites {}", rule.uid, rule.query);
        }
    }
}
