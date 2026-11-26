use super::*;

#[test]
fn les_identifiants_sont_uniques_et_prefixes() {
    let rules = rules();
    let mut uids: Vec<&str> = rules.iter().map(|r| r.uid.as_str()).collect();
    assert!(uids.iter().all(|uid| uid.starts_with("synology_")), "{uids:?}");
    uids.sort_unstable();
    let total = uids.len();
    uids.dedup();
    assert_eq!(uids.len(), total, "two Synology rules share a uid");
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
fn seul_l_echec_de_hyper_backup_reveille() {
    let rule = rules().into_iter().find(|r| r.uid == "synology_backup_failed").unwrap();
    // 0 réussi ou en cours, 1 jamais exécutée ou annulée, 2 échec.
    assert_eq!(rule.operator, Operator::Ge);
    assert_eq!(rule.threshold, 2.0);
}

#[test]
fn les_deux_seuils_de_certificat_s_emboitent() {
    let all = rules();
    let soon = all.iter().find(|r| r.uid == "synology_certificate_expiring").unwrap();
    let now = all.iter().find(|r| r.uid == "synology_certificate_expired").unwrap();
    assert_eq!(soon.query, now.query);
    assert!(now.threshold < soon.threshold);
    assert!(now.severity > soon.severity);
    assert!(soon.clear_threshold.unwrap() > soon.threshold);
}

#[test]
fn l_age_de_sauvegarde_d_un_appareil_se_regle_en_duree() {
    let rule = rules().into_iter().find(|r| r.uid == "synology_abb_device_backup_too_old").unwrap();
    // Un âge en secondes, que l'éditeur montre et fait saisir en jours.
    assert_eq!(rule.query, "dumbmonit_abb_device_last_success_seconds");
    assert_eq!(rule.unit, "s");
    assert_eq!(rule.operator, Operator::Gt);
    assert_eq!(rule.threshold, 3.0 * 86_400.0);
    assert_eq!(rule.threshold % 86_400.0, 0.0, "the default reads as whole days");
    assert!(rule.clear_threshold.is_none());
}

#[test]
fn aucune_regle_n_est_sans_duree() {
    assert!(rules().iter().all(|r| r.for_duration > Duration::ZERO));
}
