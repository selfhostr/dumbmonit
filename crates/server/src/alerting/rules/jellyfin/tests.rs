use super::*;

#[test]
fn les_regles_jellyfin_visent_des_metriques_declarees() {
    let rules = rules();
    let uids: Vec<&str> = rules.iter().map(|r| r.uid.as_str()).collect();
    assert_eq!(
        uids,
        [
            "jellyfin_library_scan_stale",
            "jellyfin_storage_almost_full",
            "jellyfin_cpu_transcoding_heavy",
            "jellyfin_failed_logins",
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
