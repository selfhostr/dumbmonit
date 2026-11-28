use super::*;

#[test]
fn les_identifiants_sont_uniques_et_prefixes() {
    let rules = rules();
    let mut uids: Vec<&str> = rules.iter().map(|r| r.uid.as_str()).collect();
    assert!(uids.iter().all(|uid| uid.starts_with("postgres_")), "{uids:?}");
    uids.sort_unstable();
    let total = uids.len();
    uids.dedup();
    assert_eq!(uids.len(), total, "two PostgreSQL rules share a uid");
}

#[test]
fn chaque_metrique_visee_est_declaree_produite() {
    for rule in rules() {
        for mot in rule.query.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if mot.starts_with("dumbmonit_") {
                assert!(PRODUCED.contains(&mot), "{} cites {mot}", rule.uid);
            }
        }
    }
}

#[test]
fn la_transaction_longue_tolere_une_sauvegarde_pg_dump() {
    let rule = rules().into_iter().find(|r| r.uid == "postgres_long_transaction").unwrap();
    assert!(rule.threshold >= 6.0 * 3600.0);
    assert!(rule.for_duration >= Duration::from_secs(600));
}

#[test]
fn le_slot_inactif_ne_sonne_pas_pour_un_slot_actif() {
    let rule = rules().into_iter().find(|r| r.uid == "postgres_slot_holding_wal").unwrap();
    assert!(rule.query.contains("replication_slot_active == 0"), "{}", rule.query);
    assert_eq!(rule.operator, Operator::Gt);
}

#[test]
fn le_taux_de_cache_a_un_plancher_de_volume() {
    let rule = rules().into_iter().find(|r| r.uid == "postgres_cache_hit_low").unwrap();
    assert!(rule.query.contains(">= 100000"), "{}", rule.query);
    assert_eq!(rule.severity, Severity::Info);
}

#[test]
fn les_regles_sont_livrees_actives_et_sans_canal_impose() {
    for rule in rules() {
        assert!(rule.enabled && rule.builtin && rule.channels.is_empty(), "{}", rule.uid);
    }
}
