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
