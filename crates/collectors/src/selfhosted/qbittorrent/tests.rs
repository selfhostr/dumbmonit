use std::collections::BTreeMap;
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};

use super::*;

// `sync/maindata` d'un qBittorrent 5.0.4 : sept torrents, dont un en
// erreur, un aux fichiers manquants et un bloqué sans source.
const MAINDATA: &str = include_str!("../testdata/qbittorrent_5.0.4/maindata.json");
const FIREWALLED: &str = include_str!("../testdata/qbittorrent_5.0.4/maindata_firewalled.json");
const VERSION: &str = include_str!("../testdata/qbittorrent_5.0.4/version.txt");

fn value(samples: &[Sample], name: &str) -> Option<f64> {
    samples.iter().find(|s| s.metric == name).map(|s| s.value)
}

fn by_state(samples: &[Sample], state: &str) -> Option<f64> {
    samples
        .iter()
        .find(|s| {
            s.metric == "qbittorrent_torrents"
                && s.labels.get("state").map(String::as_str) == Some(state)
        })
        .map(|s| s.value)
}

#[test]
fn les_etats_des_deux_generations_d_api_se_regroupent() {
    assert_eq!(state_group("pausedUP"), "paused");
    assert_eq!(state_group("stoppedUP"), "paused");
    assert_eq!(state_group("missingFiles"), "error");
    assert_eq!(state_group("forcedMetaDL"), "downloading");
    assert_eq!(state_group("stalledUP"), "seeding");
    assert_eq!(state_group("nouveau"), "other");
}

#[test]
fn les_torrents_se_comptent_par_etat_et_les_erreurs_se_nomment() {
    let samples = samples("v5.0.4", &serde_json::from_str(MAINDATA).unwrap(), 0);
    assert_eq!(value(&samples, "qbittorrent_torrents_total"), Some(7.0));
    assert_eq!(value(&samples, "qbittorrent_torrents_errored"), Some(2.0));
    assert_eq!(by_state(&samples, "downloading"), Some(1.0));
    assert_eq!(by_state(&samples, "stalled_download"), Some(1.0));
    assert_eq!(by_state(&samples, "seeding"), Some(1.0));
    assert_eq!(by_state(&samples, "paused"), Some(1.0));
    assert_eq!(by_state(&samples, "queued"), Some(1.0));
    let named: Vec<&Sample> =
        samples.iter().filter(|s| s.metric == "qbittorrent_torrent_error").collect();
    assert_eq!(named.len(), 2);
    assert!(named.iter().any(|s| s.labels["torrent"] == "Docu.S01E01.720p.mkv"));
    assert!(named.iter().any(|s| s.labels["state"] == "missingFiles"));
}

#[test]
fn vitesses_connectivite_et_espace_libre() {
    let samples = samples("v5.0.4", &serde_json::from_str(MAINDATA).unwrap(), 0);
    assert_eq!(value(&samples, "qbittorrent_connected"), Some(1.0));
    assert_eq!(value(&samples, "qbittorrent_download_speed_bytes"), Some(4_194_304.0));
    assert_eq!(value(&samples, "qbittorrent_upload_speed_bytes"), Some(65_536.0));
    assert_eq!(value(&samples, "qbittorrent_free_space_bytes"), Some(268_435_456_000.0));
    assert_eq!(value(&samples, "qbittorrent_dht_nodes"), Some(312.0));
}

#[test]
fn un_port_ferme_se_voit_comme_une_connectivite_perdue() {
    let samples = samples("v5.0.4", &serde_json::from_str(FIREWALLED).unwrap(), 0);
    assert_eq!(value(&samples, "qbittorrent_connected"), Some(0.0));
    let connected = samples.iter().find(|s| s.metric == "qbittorrent_connected").unwrap();
    assert_eq!(connected.labels["status"], "firewalled");
    assert_eq!(value(&samples, "qbittorrent_torrents_total"), Some(0.0));
    assert_eq!(value(&samples, "qbittorrent_torrents_errored"), Some(0.0));
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}

fn target(address: String, credential: Credential) -> Target {
    Target {
        id: 3,
        name: "torrents".into(),
        address,
        kind: "qbittorrent".into(),
        profile_id: None,
        parent_id: None,
        interval: Duration::from_secs(60),
        enabled: true,
        tags: BTreeMap::new(),
        credential,
        group_name: String::new(),
        position: 0,
    }
}

fn login_ok() -> impl IntoResponse {
    ([(header::SET_COOKIE, "SID=abc123; HttpOnly; SameSite=Strict; path=/")], "Ok.")
}

fn app() -> Router {
    let authorised = |headers: &HeaderMap| {
        headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) == Some("SID=abc123")
    };
    Router::new()
        .route(
            "/api/v2/auth/login",
            post(|body: String| async move {
                if body.contains("password=secret") {
                    login_ok().into_response()
                } else {
                    "Fails.".into_response()
                }
            }),
        )
        .route(
            "/api/v2/app/version",
            get(move |headers: HeaderMap| async move {
                if authorised(&headers) {
                    (axum::http::StatusCode::OK, VERSION)
                } else {
                    (axum::http::StatusCode::FORBIDDEN, "Forbidden")
                }
            }),
        )
        .route("/api/v2/sync/maindata", get(|| async { MAINDATA }))
}

fn login(user: &str, password: &str) -> Credential {
    Credential::UsernamePassword { username: user.into(), password: password.into() }
}

#[tokio::test]
async fn la_session_ouverte_par_cookie_donne_les_mesures() {
    let base = serve(app()).await;
    let samples =
        QbittorrentCollector::new().probe(&target(base, login("admin", "secret"))).await.unwrap();
    assert_eq!(value(&samples, "qbittorrent_torrents_errored"), Some(2.0));
    let info = samples.iter().find(|s| s.metric == "qbittorrent_version_info").unwrap();
    assert_eq!(info.labels["version"], "v5.0.4");
}

#[tokio::test]
async fn un_mauvais_mot_de_passe_n_est_pas_une_panne() {
    let base = serve(app()).await;
    let error = QbittorrentCollector::new()
        .probe(&target(base, login("admin", "mauvais")))
        .await
        .unwrap_err();
    assert!(matches!(error, ProbeError::Auth(_)));
    assert!(!error.means_down());
}

#[tokio::test]
async fn qbittorrent_5_repond_401_a_un_mauvais_mot_de_passe_et_le_message_parle_de_compte() {
    // Observé sur qBittorrent 5.3.0 : le login échoué rend 401 (et non
    // 200 « Fails. » comme les versions 4.x).
    let app = Router::new()
        .route("/api/v2/auth/login", post(|| async { (axum::http::StatusCode::UNAUTHORIZED, "") }));
    let base = serve(app).await;
    let error = QbittorrentCollector::new()
        .probe(&target(base, login("admin", "mauvais")))
        .await
        .unwrap_err();
    assert!(matches!(error, ProbeError::Auth(_)));
    assert!(!error.means_down());
    let message = error.to_string();
    assert!(message.contains("user name and password"), "{message}");
    assert!(!message.contains("token"), "{message}");
}

#[tokio::test]
async fn sans_identifiants_et_sans_contournement_l_acces_est_refuse() {
    let base = serve(app()).await;
    let error =
        QbittorrentCollector::new().probe(&target(base, Credential::None)).await.unwrap_err();
    assert!(matches!(error, ProbeError::Auth(_)));
    // Un compte à saisir, pas un jeton à vérifier.
    let message = error.to_string();
    assert!(message.contains("user name and password"), "{message}");
    assert!(!message.contains("token"), "{message}");
}

#[tokio::test]
async fn un_jeton_n_est_pas_un_identifiant_qbittorrent() {
    let credential = Credential::ApiToken { token: "x".into() };
    let error = QbittorrentCollector::new()
        .probe(&target("http://127.0.0.1:1".into(), credential))
        .await
        .unwrap_err();
    assert!(matches!(error, ProbeError::Config(_)));
}
