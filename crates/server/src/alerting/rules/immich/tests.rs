use super::*;

#[test]
fn les_regles_immich_visent_des_metriques_declarees() {
    let rules = rules();
    let uids: Vec<&str> = rules.iter().map(|r| r.uid.as_str()).collect();
    assert_eq!(
        uids,
        [
            "immich_jobs_failed",
            "immich_ml_failing",
            "immich_maintenance_mode",
            "immich_user_quota_almost_full",
            "immich_db_backup_stale",
        ]
    );
    for rule in &rules {
        assert!(rule.builtin && rule.enabled);
        for mot in rule.query.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if mot.starts_with("dumbmonit_") {
                assert!(PRODUCED.contains(&mot), "{}: {mot}", rule.uid);
            }
        }
    }
}

#[test]
fn les_deux_regles_d_echec_se_partagent_les_files_sans_recouvrement() {
    let rules = rules();
    let generic = &rules[0].query;
    let ml = &rules[1].query;
    assert!(generic.contains("queue!~") && ml.contains("queue=~"));
    // Même liste de files des deux côtés.
    let list = |q: &str| q.split('"').nth(1).unwrap().to_string();
    assert_eq!(list(generic), list(ml));
}
