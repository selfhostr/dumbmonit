use dumbmonit_proto::Credential;

use super::*;
use crate::uptime::tags::test_support::cible;
use fake::{Behaviour, FakeServer};

const SECRET: &str = "testing123";

fn target(address: &str, secret: &str, password: &str, tags: &[(&str, &str)]) -> Target {
    let mut all = vec![("allow_private_targets", "true"), ("timeout_seconds", "2")];
    all.extend_from_slice(tags);
    let mut target = cible("radius", address, &all);
    target.credential = Credential::Radius {
        shared_secret: secret.into(),
        username: "probe".into(),
        password: password.into(),
    };
    target
}

async fn probe(target: &Target) -> Vec<Sample> {
    RadiusCollector::new().probe(target).await.expect("une mesure, pas une erreur")
}

fn metric(samples: &[Sample], name: &str) -> Option<f64> {
    samples.iter().find(|s| s.metric == name).map(|s| s.value)
}

fn reason(samples: &[Sample]) -> Option<String> {
    samples
        .iter()
        .find(|s| s.metric == "probe_failure_info")
        .and_then(|s| s.labels.get("reason").cloned())
}

#[test]
fn le_collecteur_annonce_son_type() {
    assert_eq!(RadiusCollector::new().kind(), "radius");
}

#[tokio::test]
async fn un_compte_accepte_reussit_et_publie_le_code() {
    let server = FakeServer::spawn(SECRET, "probe", "pw", Behaviour::Normal).await;
    let samples = probe(&target(&server.address, SECRET, "pw", &[])).await;
    assert_eq!(metric(&samples, "probe_success"), Some(1.0));
    assert_eq!(metric(&samples, "probe_radius_response_code"), Some(2.0));
    assert!(metric(&samples, "probe_radius_response_seconds").is_some());
    assert!(reason(&samples).is_none());
    let seen = server.last_request().expect("la requête a été reçue");
    assert_eq!(seen.username, "probe");
    assert_eq!(seen.password, "pw", "le mot de passe est démasqué avec le bon secret");
    assert_eq!(seen.nas_identifier, "dumbmonit");
    assert!(seen.signature_valid && seen.signature_first);
}

#[tokio::test]
async fn un_mot_de_passe_refuse_se_distingue_dune_panne() {
    let server = FakeServer::spawn(SECRET, "probe", "pw", Behaviour::Normal).await;
    let samples = probe(&target(&server.address, SECRET, "mauvais", &[])).await;
    assert_eq!(metric(&samples, "probe_success"), Some(0.0));
    assert_eq!(reason(&samples).as_deref(), Some("auth"));
    assert_eq!(metric(&samples, "probe_radius_response_code"), Some(3.0));
}

#[tokio::test]
async fn un_secret_faux_est_signale_comme_tel() {
    // Le serveur signe avec le sien : la signature ne se vérifie pas chez nous.
    let server = FakeServer::spawn("le-vrai-secret", "probe", "pw", Behaviour::Normal).await;
    let samples = probe(&target(&server.address, "un-autre-secret", "pw", &[])).await;
    assert_eq!(metric(&samples, "probe_success"), Some(0.0));
    assert_eq!(reason(&samples).as_deref(), Some("bad_secret"));
}

#[tokio::test]
async fn un_defi_est_un_echec_de_protocole_lisible() {
    let server = FakeServer::spawn(SECRET, "probe", "pw", Behaviour::Challenge).await;
    let samples = probe(&target(&server.address, SECRET, "pw", &[])).await;
    assert_eq!(metric(&samples, "probe_success"), Some(0.0));
    assert_eq!(reason(&samples).as_deref(), Some("protocol"));
}

#[tokio::test]
async fn un_serveur_muet_produit_un_echec_mesure_et_non_une_erreur() {
    let server = FakeServer::spawn(SECRET, "probe", "pw", Behaviour::Silent).await;
    let target = target(&server.address, SECRET, "pw", &[("timeout_seconds", "1")]);
    let samples = probe(&target).await;
    assert_eq!(metric(&samples, "probe_success"), Some(0.0));
    assert_eq!(reason(&samples).as_deref(), Some("timeout"));
}

/// Un datagramme d'une autre requête arrive avant la vraie réponse : il est
/// ignoré, la sonde attend la bonne.
#[tokio::test]
async fn une_reponse_a_une_autre_requete_est_ignoree() {
    let server = FakeServer::spawn(SECRET, "probe", "pw", Behaviour::WrongIdFirst).await;
    let samples = probe(&target(&server.address, SECRET, "pw", &[])).await;
    assert_eq!(metric(&samples, "probe_success"), Some(1.0));
}

#[tokio::test]
async fn un_paquet_qui_nest_pas_du_radius_est_un_echec_de_protocole() {
    let server = FakeServer::spawn(SECRET, "probe", "pw", Behaviour::Garbage).await;
    let samples = probe(&target(&server.address, SECRET, "pw", &[])).await;
    assert_eq!(metric(&samples, "probe_success"), Some(0.0));
    assert_eq!(reason(&samples).as_deref(), Some("protocol"));
}

#[tokio::test]
async fn lidentifiant_de_nas_configure_part_dans_la_requete() {
    let server = FakeServer::spawn(SECRET, "probe", "pw", Behaviour::Normal).await;
    let target = target(&server.address, SECRET, "pw", &[("nas_identifier", "portail-1")]);
    probe(&target).await;
    assert_eq!(server.last_request().unwrap().nas_identifier, "portail-1");
}

#[tokio::test]
async fn la_boucle_locale_est_refusee_sans_loption() {
    let mut target = cible("radius", "127.0.0.1", &[("timeout_seconds", "2")]);
    target.credential = Credential::Radius {
        shared_secret: SECRET.into(),
        username: "probe".into(),
        password: "pw".into(),
    };
    let error = RadiusCollector::new().probe(&target).await.unwrap_err();
    assert!(matches!(error, ProbeError::Config(_)), "{error}");
    assert!(!error.means_down());
    assert!(error.to_string().contains("allow_private_targets"), "{error}");
}

#[tokio::test]
async fn sans_identifiant_radius_la_sonde_demande_den_fournir_un() {
    let target = cible("radius", "radius.lan", &[]);
    let error = RadiusCollector::new().probe(&target).await.unwrap_err();
    assert!(matches!(error, ProbeError::Config(_)), "{error}");
}
