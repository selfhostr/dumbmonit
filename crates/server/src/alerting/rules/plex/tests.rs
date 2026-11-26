use super::*;

#[test]
fn les_regles_plex_visent_des_metriques_declarees() {
    let rules = rules();
    let uids: Vec<&str> = rules.iter().map(|r| r.uid.as_str()).collect();
    assert_eq!(
        uids,
        [
            "plex_transcoder_too_slow",
            "plex_transcode_error",
            "plex_remote_access_failed",
            "plex_signin_lost",
            "plex_database_backup_off",
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
