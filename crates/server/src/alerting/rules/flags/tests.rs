use super::*;
use crate::alerting::model::Operator;
use crate::alerting::rules::builtin_rules;

#[test]
fn chaque_drapeau_designe_une_regle_livree() {
    let rules = builtin_rules();
    for uid in FLAG_RULES {
        assert!(rules.iter().any(|rule| rule.uid == *uid), "{uid} is not a built-in rule");
    }
    let mut uids = FLAG_RULES.to_vec();
    uids.sort_unstable();
    let total = uids.len();
    uids.dedup();
    assert_eq!(uids.len(), total, "a flag rule is listed twice");
}

#[test]
fn un_drapeau_compare_a_zero_ou_un_sans_hysteresis() {
    for rule in builtin_rules().iter().filter(|rule| rule.unit == FLAG_UNIT) {
        let bit = match rule.operator {
            Operator::Gt | Operator::Le => rule.threshold == 0.0,
            Operator::Lt | Operator::Ge => rule.threshold == 1.0,
        };
        assert!(bit, "{} reads a 0/1 signal but compares it with {}", rule.uid, rule.threshold);
        assert!(rule.clear_threshold.is_none(), "{} has a clear threshold", rule.uid);
    }
}

#[test]
fn les_regles_livrees_marquees_portent_l_unite_bool() {
    let rules = builtin_rules();
    let overdue = rules.iter().find(|r| r.uid == "synology_abb_device_overdue").unwrap();
    assert_eq!(overdue.unit, FLAG_UNIT);
    // Une règle qui compte garde son seuil réglable.
    let failing = rules.iter().find(|r| r.uid == "synology_abb_device_failing").unwrap();
    assert_eq!(failing.unit, "");
    assert!(!FLAG_RULES.contains(&"pve_tasks_failed"));
}
