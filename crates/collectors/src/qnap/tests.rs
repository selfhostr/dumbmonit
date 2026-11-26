use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::Form;
use axum::http::{StatusCode, Uri};
use axum::routing::{get, post};

use super::*;
use crate::rest::test_support::{find as find_sample, serve, value};
use crate::uptime::tags::test_support::cible;

// Réponses reconstruites d'après ce que lisent `python-qnapstats` et
// l'intégration Home Assistant `qnap` (noms des balises), pas capturées
// sur un NAS : un NAS de quatre baies dont un volume presque plein, un
// disque en avertissement, un disque anormal, un emplacement vide, une
// mise à jour de QTS proposée.
const AUTH_LOGIN: &str = include_str!("testdata/auth_login.xml");
const AUTH_FAILED: &str = include_str!("testdata/auth_failed.xml");
const AUTH_2SV: &str = include_str!("testdata/auth_2sv.xml");
const SYSINFO_XML: &str = include_str!("testdata/sysinfo.xml");
const VOLUMES_XML: &str = include_str!("testdata/volumes.xml");
const DISKS_XML: &str = include_str!("testdata/disks.xml");
const FIRMWARE_UPDATE: &str = include_str!("testdata/firmware_update.xml");
const FIRMWARE_NONE: &str = include_str!("testdata/firmware_none.xml");

const SID: &str = "q2x9k1abcd";

fn replies() -> Replies {
    Replies {
        login: parse_login(AUTH_LOGIN).unwrap(),
        sysinfo: SYSINFO_XML.to_string(),
        volumes: Some(VOLUMES_XML.to_string()),
        disks: Some(DISKS_XML.to_string()),
        firmware: Some(FIRMWARE_UPDATE.to_string()),
    }
}

#[test]
fn la_connexion() {
    let login = parse_login(AUTH_LOGIN).unwrap();
    assert_eq!(login.sid, SID);
    assert_eq!((login.model.as_str(), login.version.as_str()), ("TS-453D", "5.1.7"));
    assert!(matches!(parse_login(AUTH_FAILED), Err(ProbeError::Auth(_))));
    assert!(matches!(parse_login(AUTH_2SV), Err(ProbeError::Config(_))));
    assert!(matches!(parse_login("<html>login</html>"), Err(ProbeError::Protocol(_))));
    assert!(matches!(parse_login("<other/>"), Err(ProbeError::Protocol(_))));
}

#[test]
fn systeme_volumes_disques_micrologiciel() {
    let s = samples(&replies(), 0);
    let info = find_sample(&s, "qnap_version_info", &[]).unwrap();
    assert_eq!(
        (info.labels["version"].as_str(), info.labels["model"].as_str()),
        ("5.1.7", "TS-453D")
    );
    assert_eq!(value(&s, "qnap_cpu_usage_percent", &[]), 7.8);
    assert_eq!(value(&s, "qnap_cpu_temperature_celsius", &[]), 52.0);
    assert_eq!(value(&s, "qnap_system_temperature_celsius", &[]), 41.0);
    assert_eq!(value(&s, "qnap_memory_used_percent", &[]), 75.0);
    assert_eq!(value(&s, "qnap_memory_total_bytes", &[]), 8000.0 * MIB);
    assert_eq!(value(&s, "qnap_uptime_seconds", &[]), 1_053_065.0);

    assert_eq!(value(&s, "qnap_volumes", &[]), 2.0);
    assert!(
        (value(&s, "qnap_volume_used_percent", &[("volume", "DataVol1")]) - 90.0).abs() < 1e-6
    );
    assert!(
        (value(&s, "qnap_volume_used_percent", &[("volume", "Backups")]) - 25.0).abs() < 1e-6
    );

    assert_eq!(value(&s, "qnap_disks", &[]), 3.0, "the empty slot is left out");
    assert_eq!(value(&s, "qnap_disks_unhealthy", &[]), 2.0);
    assert_eq!(value(&s, "qnap_disk_ok", &[("disk", "disk 1")]), 1.0);
    assert_eq!(value(&s, "qnap_disk_warning", &[("disk", "disk 2")]), 1.0);
    assert_eq!(value(&s, "qnap_disk_ok", &[("disk", "disk 3")]), 0.0);
    assert_eq!(value(&s, "qnap_disk_temperature_celsius", &[("disk", "disk 1")]), 36.0);

    assert_eq!(value(&s, "qnap_firmware_update_available", &[]), 1.0);
    assert_eq!(
        find_sample(&s, "qnap_firmware_latest_info", &[]).unwrap().labels["version"],
        "5.2.0.2802"
    );
}

#[test]
fn sans_lectures_facultatives() {
    let mut r = replies();
    r.volumes = None;
    r.disks = None;
    r.firmware = Some(FIRMWARE_NONE.to_string());
    let s = samples(&r, 0);
    assert!(find_sample(&s, "qnap_volumes", &[]).is_none());
    assert!(find_sample(&s, "qnap_disks", &[]).is_none());
    assert_eq!(value(&s, "qnap_firmware_update_available", &[]), 0.0);
    assert!(find_sample(&s, "qnap_firmware_latest_info", &[]).is_none());
    assert_eq!(value(&s, "qnap_cpu_usage_percent", &[]), 7.8);
}

#[test]
fn etats_smart() {
    assert_eq!(health("OK"), Health::Good);
    assert_eq!(health(" Warning "), Health::Warning);
    assert_eq!(health("Abnormal"), Health::Failed);
}

#[test]
fn l_identifiant_de_session_est_retire_des_erreurs() {
    let error = ProbeError::Unreachable(format!("/x?sid={SID}: connection refused"));
    let message = scrub(error, SID).to_string();
    assert!(!message.contains(SID) && message.contains("***"), "{message}");
}

fn fake(logins: Arc<AtomicUsize>, logouts: Arc<AtomicUsize>, broken: bool) -> Router {
    Router::new()
        .route(
            LOGIN,
            post(move |Form(form): Form<HashMap<String, String>>| {
                let logins = logins.clone();
                async move {
                    logins.fetch_add(1, Ordering::SeqCst);
                    // « s3cret » en base64.
                    let good = form.get("user").map(String::as_str) == Some("dumbmonit")
                        && form.get("pwd").map(String::as_str) == Some("czNjcmV0");
                    if good { AUTH_LOGIN } else { AUTH_FAILED }
                }
            }),
        )
        .fallback(get(move |uri: Uri| {
            let logouts = logouts.clone();
            async move {
                let query = uri.query().unwrap_or_default();
                if !query.contains(&format!("sid={SID}")) {
                    return (StatusCode::OK, AUTH_FAILED);
                }
                match uri.path() {
                    "/cgi-bin/authLogout.cgi" => {
                        logouts.fetch_add(1, Ordering::SeqCst);
                        (StatusCode::OK, "<QDocRoot version=\"1.0\"></QDocRoot>")
                    }
                    "/cgi-bin/management/manaRequest.cgi" if broken => {
                        (StatusCode::INTERNAL_SERVER_ERROR, "")
                    }
                    "/cgi-bin/management/manaRequest.cgi" => (StatusCode::OK, SYSINFO_XML),
                    "/cgi-bin/management/chartReq.cgi" => (StatusCode::OK, VOLUMES_XML),
                    // Compte sans droit sur les disques, route absente pour le reste.
                    "/cgi-bin/disk/disk_manage.cgi" => (StatusCode::FORBIDDEN, ""),
                    _ => (StatusCode::NOT_FOUND, ""),
                }
            }
        }))
}

fn target(address: &str, password: &str) -> Target {
    let mut target = cible("qnap", &format!("http://{address}"), &[]);
    target.credential = Credential::UsernamePassword {
        username: "dumbmonit".into(),
        password: password.into(),
    };
    target
}

#[tokio::test]
async fn interrogation_complete_et_deconnexion() {
    let (logins, logouts) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let address = serve(fake(logins.clone(), logouts.clone(), false)).await;
    let s = QnapCollector::new().probe(&target(&address, "s3cret")).await.unwrap();
    assert_eq!(value(&s, "qnap_volumes", &[]), 2.0);
    assert!(find_sample(&s, "qnap_disks", &[]).is_none(), "refused optional read");
    assert_eq!(logins.load(Ordering::SeqCst), 1);
    assert_eq!(logouts.load(Ordering::SeqCst), 1, "the session is closed");
}

#[tokio::test]
async fn mauvais_mot_de_passe_et_panne_en_cours_de_route() {
    let (logins, logouts) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let address = serve(fake(logins.clone(), logouts.clone(), true)).await;

    let error = QnapCollector::new().probe(&target(&address, "wrong")).await.unwrap_err();
    assert!(matches!(error, ProbeError::Auth(_)), "{error}");
    assert!(!error.means_down());

    // Une erreur serveur sur la lecture obligatoire : panne, sans l'identifiant.
    let error = QnapCollector::new().probe(&target(&address, "s3cret")).await.unwrap_err();
    assert!(error.means_down(), "{error}");
    assert!(!error.to_string().contains(SID), "{error}");
    assert_eq!(logouts.load(Ordering::SeqCst), 1, "closed even after a failed read");
}
