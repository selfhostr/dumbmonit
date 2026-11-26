use std::sync::{Arc, Mutex};

use dumbmonit_proto::Credential;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use super::*;
use crate::uptime::tags::test_support::cible;

fn metric(samples: &[Sample], name: &str) -> Option<f64> {
    samples.iter().find(|s| s.metric == name).map(|s| s.value)
}

fn reason(samples: &[Sample]) -> Option<String> {
    samples
        .iter()
        .find(|s| s.metric == "probe_failure_info")
        .and_then(|s| s.labels.get("reason").cloned())
}

/// Faux serveur IMAP. `script` associe une commande à la réponse envoyée, où
/// `{tag}` est remplacé par l'étiquette du client. Rend le port et les lignes
/// reçues.
async fn faux_imap(
    greeting: &'static str,
    script: Vec<(&'static str, &'static str)>,
) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(Mutex::new(Vec::new()));
    let log = received.clone();
    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read, mut write) = socket.into_split();
        write.write_all(format!("{greeting}\r\n").as_bytes()).await.unwrap();
        let mut lines = BufReader::new(read).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log.lock().unwrap().push(line.clone());
            let mut words = line.splitn(3, ' ');
            let tag = words.next().unwrap_or("");
            let command = words.next().unwrap_or("").to_ascii_uppercase();
            let answer = script
                .iter()
                .find(|(name, _)| *name == command)
                .map(|(_, answer)| answer.replace("{tag}", tag))
                .unwrap_or_else(|| format!("{tag} BAD unknown command\r\n"));
            if write.write_all(answer.as_bytes()).await.is_err() || command == "LOGOUT" {
                break;
            }
        }
    });
    (port, received)
}

fn cible_locale(port: u16, tags: &[(&str, &str)]) -> Target {
    let mut all =
        vec![("timeout_seconds", "3"), ("allow_private_targets", "true"), ("security", "none")];
    all.extend_from_slice(tags);
    cible("imap", &format!("127.0.0.1:{port}"), &all)
}

fn avec_identifiants(mut target: Target, password: &str) -> Target {
    target.credential =
        Credential::UsernamePassword { username: "monit".into(), password: password.into() };
    target
}

#[test]
fn le_collecteur_annonce_son_type() {
    assert_eq!(ImapCollector::new().kind(), "imap");
}

#[test]
fn les_chaines_sont_citees_selon_imap() {
    assert_eq!(quote("simple"), "\"simple\"");
    assert_eq!(quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
}

#[tokio::test]
async fn la_boucle_locale_est_refusee_sans_loption() {
    let target = cible("imap", "127.0.0.1:143", &[("timeout_seconds", "2")]);
    let error = ImapCollector::new().probe(&target).await.unwrap_err();
    assert!(matches!(error, ProbeError::Config(_)), "{error}");
    assert!(!error.means_down());
    assert!(error.to_string().contains("allow_private_targets"), "{error}");
}

#[tokio::test]
async fn une_banniere_seule_suffit_sans_identifiants() {
    let (port, received) = faux_imap("* OK IMAP4rev1 ready", vec![]).await;
    let samples = ImapCollector::new().probe(&cible_locale(port, &[])).await.unwrap();
    assert_eq!(metric(&samples, "probe_success"), Some(1.0), "{:?}", reason(&samples));
    assert!(metric(&samples, "probe_imap_greeting_seconds").is_some());
    assert!(metric(&samples, "probe_imap_authenticated").is_none());
    assert!(received.lock().unwrap().iter().all(|line| !line.contains("LOGIN")));
}

#[tokio::test]
async fn une_connexion_reussie_est_authentifiee_et_citee_correctement() {
    let (port, received) =
        faux_imap("* OK ready", vec![("LOGIN", "{tag} OK LOGIN completed\r\n")]).await;
    let target = avec_identifiants(cible_locale(port, &[]), "pa\"ss\\w");
    let samples = ImapCollector::new().probe(&target).await.unwrap();
    assert_eq!(metric(&samples, "probe_success"), Some(1.0), "{:?}", reason(&samples));
    assert_eq!(metric(&samples, "probe_imap_authenticated"), Some(1.0));
    assert!(metric(&samples, "probe_imap_login_seconds").is_some());
    let lines = received.lock().unwrap();
    assert!(lines.contains(&"a002 LOGIN \"monit\" \"pa\\\"ss\\\\w\"".to_string()), "{lines:?}");
}

#[tokio::test]
async fn un_mot_de_passe_refuse_se_distingue_dune_panne() {
    let (port, _) = faux_imap(
        "* OK ready",
        vec![("LOGIN", "{tag} NO [AUTHENTICATIONFAILED] Authentication failed.\r\n")],
    )
    .await;
    let target = avec_identifiants(cible_locale(port, &[]), "faux");
    let samples = ImapCollector::new().probe(&target).await.unwrap();
    assert_eq!(metric(&samples, "probe_success"), Some(0.0));
    assert_eq!(reason(&samples).as_deref(), Some("auth"));
}

#[tokio::test]
async fn un_serveur_indisponible_nest_pas_une_erreur_de_compte() {
    let (port, _) = faux_imap(
        "* OK ready",
        vec![("LOGIN", "* OK noise\r\n{tag} NO [UNAVAILABLE] backend down\r\n")],
    )
    .await;
    let target = avec_identifiants(cible_locale(port, &[]), "x");
    let samples = ImapCollector::new().probe(&target).await.unwrap();
    assert_eq!(reason(&samples).as_deref(), Some("protocol"));
}

#[tokio::test]
async fn preauth_dispense_de_login() {
    let (port, received) = faux_imap("* PREAUTH ready", vec![]).await;
    let target = avec_identifiants(cible_locale(port, &[]), "x");
    let samples = ImapCollector::new().probe(&target).await.unwrap();
    assert_eq!(metric(&samples, "probe_success"), Some(1.0));
    assert_eq!(metric(&samples, "probe_imap_authenticated"), Some(1.0));
    assert!(received.lock().unwrap().iter().all(|line| !line.contains("LOGIN")));
}

/// Le point de sécurité : si `STARTTLS` est refusé, le mot de passe ne part pas.
#[tokio::test]
async fn starttls_refuse_ninterrompt_la_sonde_avant_tout_identifiant() {
    let (port, received) = faux_imap(
        "* OK ready",
        vec![("STARTTLS", "{tag} NO TLS not available\r\n"), ("LOGIN", "{tag} OK logged in\r\n")],
    )
    .await;
    let target = avec_identifiants(cible_locale(port, &[("security", "starttls")]), "secret");
    let samples = ImapCollector::new().probe(&target).await.unwrap();
    assert_eq!(metric(&samples, "probe_success"), Some(0.0));
    assert_eq!(reason(&samples).as_deref(), Some("tls"));
    assert!(received.lock().unwrap().iter().all(|line| !line.contains("LOGIN")));
}

#[tokio::test]
async fn un_service_qui_ne_parle_pas_imap_est_signale() {
    let (port, _) = faux_imap("SSH-2.0-OpenSSH_9.6", vec![]).await;
    let samples = ImapCollector::new().probe(&cible_locale(port, &[])).await.unwrap();
    assert_eq!(reason(&samples).as_deref(), Some("protocol"));
}

#[tokio::test]
async fn un_serveur_injoignable_produit_un_echantillon_a_zero() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let samples = ImapCollector::new().probe(&cible_locale(port, &[])).await.unwrap();
    assert_eq!(metric(&samples, "probe_success"), Some(0.0));
    assert_eq!(reason(&samples).as_deref(), Some("connect"));
}
