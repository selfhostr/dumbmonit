use super::*;

#[test]
fn les_identifiants_sont_uniques_et_prefixes() {
    let rules = rules();
    let mut uids: Vec<&str> = rules.iter().map(|r| r.uid.as_str()).collect();
    assert!(uids.iter().all(|uid| uid.starts_with("nextcloud_")), "{uids:?}");
    uids.sort_unstable();
    let total = uids.len();
    uids.dedup();
    assert_eq!(uids.len(), total, "two Nextcloud rules share a uid");
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
fn la_limite_de_memoire_illimitee_ne_sonne_pas() {
    let rule = rules().into_iter().find(|r| r.uid == "nextcloud_php_memory_low").unwrap();
    // PHP note l'absence de limite -1 : la requête l'écarte avant la comparaison.
    assert!(rule.query.ends_with("> 0"), "{}", rule.query);
    assert_eq!(rule.operator, Operator::Lt);
}

#[test]
fn le_temoin_de_cron_attend_deux_semaines() {
    let rule = rules().into_iter().find(|r| r.uid == "nextcloud_background_jobs_stale").unwrap();
    assert_eq!(rule.threshold, 14.0 * 86400.0);
}

#[test]
fn aucune_regle_n_est_sans_duree() {
    assert!(rules().iter().all(|r| r.for_duration > Duration::ZERO));
}
