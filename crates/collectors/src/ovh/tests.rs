use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};

use super::*;
use crate::rest::test_support::{find, serve, value};
use crate::uptime::tags::test_support::cible;

const VPS_RUNNING: &str = include_str!("testdata/vps_running.json");
const VPS_STOPPED: &str = include_str!("testdata/vps_stopped.json");
const DEDICATED_OK: &str = include_str!("testdata/dedicated_ok.json");
const DEDICATED_ERROR: &str = include_str!("testdata/dedicated_error.json");

const TIME: i64 = 1_700_000_000;

/// Ce que le faux serveur d'OVHcloud répond : `mode` choisit le scénario.
struct World {
    mode: &'static str,
}

const NOT_GRANTED: &str =
    r#"{"class":"Client::Forbidden","message":"This call has not been granted"}"#;

/// Un faux serveur qui vérifie la signature de chaque requête, comme le vrai.
async fn fake(
    State(world): State<Arc<World>>,
    headers: HeaderMap,
    uri: Uri,
) -> (StatusCode, String) {
    let path = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    if path == "/1.0/auth/time" {
        return (StatusCode::OK, TIME.to_string());
    }
    if world.mode == "down" {
        return (StatusCode::SERVICE_UNAVAILABLE, "maintenance".to_string());
    }
    let header = |name: &str| {
        headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string()
    };
    let host = header("host");
    let url = format!("http://{host}{path}");
    let expected = signature("secret", "consumer", "GET", &url, "", TIME);
    if header("x-ovh-signature") != expected
        || header("x-ovh-application") != "appkey"
        || header("x-ovh-consumer") != "consumer"
        || header("x-ovh-timestamp") != TIME.to_string()
    {
        return (
            StatusCode::BAD_REQUEST,
            r#"{"class":"Client::BadRequest","message":"Invalid signature"}"#.to_string(),
        );
    }
    if world.mode == "revoked" {
        return (
            StatusCode::FORBIDDEN,
            r#"{"class":"Client::Forbidden","message":"This credential is not valid"}"#.to_string(),
        );
    }
    let ok = |body: &str| (StatusCode::OK, body.to_string());
    let forbidden = || (StatusCode::FORBIDDEN, NOT_GRANTED.to_string());
    let tasks_path = path.contains("/tasks?") || path.contains("/task?");
    if world.mode == "tasks_denied" && tasks_path {
        return forbidden();
    }
    match path {
        "/1.0/vps" if world.mode == "all_denied" => forbidden(),
        "/1.0/dedicated/server" if matches!(world.mode, "all_denied" | "dedicated_denied") => {
            forbidden()
        }
        "/1.0/vps" => ok(r#"["vps-aaa111.vps.ovh.net","vps-bbb222.vps.ovh.net"]"#),
        "/1.0/vps/vps-aaa111.vps.ovh.net" => ok(VPS_RUNNING),
        "/1.0/vps/vps-bbb222.vps.ovh.net" => ok(VPS_STOPPED),
        "/1.0/vps/vps-bbb222.vps.ovh.net/tasks?state=todo" => ok("[2001]"),
        "/1.0/vps/vps-aaa111.vps.ovh.net/serviceInfos" => {
            ok(r#"{"status":"ok","expiration":"2099-01-01","renew":{"automatic":true}}"#)
        }
        "/1.0/dedicated/server" => ok(r#"["ns100.ip-1-2-3.eu","ns200.ip-1-2-4.eu"]"#),
        "/1.0/dedicated/server/ns100.ip-1-2-3.eu" => ok(DEDICATED_OK),
        "/1.0/dedicated/server/ns200.ip-1-2-4.eu" => ok(DEDICATED_ERROR),
        "/1.0/dedicated/server/ns100.ip-1-2-3.eu/task?status=doing" => ok("[55,56]"),
        _ if tasks_path => ok("[]"),
        _ => (StatusCode::NOT_FOUND, r#"{"message":"not found"}"#.to_string()),
    }
}

async fn serveur(mode: &'static str) -> String {
    let app = Router::new().fallback(fake).with_state(Arc::new(World { mode }));
    serve(app).await
}

fn cible_ovh(address: &str, password: &str) -> Target {
    let mut target = cible("ovh", &format!("http://{address}"), &[]);
    target.credential =
        Credential::UsernamePassword { username: "appkey".into(), password: password.into() };
    target
}

#[test]
fn la_signature_suit_la_formule_d_ovh() {
    // SHA-1 de « secret+consumer+GET+http://127.0.0.1:1/1.0/vps++1700000000 ».
    assert_eq!(
        signature("secret", "consumer", "GET", "http://127.0.0.1:1/1.0/vps", "", TIME),
        "$1$fb90becef839b1b73885fd61ab5685df69364307"
    );
}

#[test]
fn le_mot_de_passe_porte_secret_et_cle_de_consommateur() {
    let ok = Credential::UsernamePassword { username: "ak".into(), password: "as:ck".into() };
    let parsed = keys(&ok).unwrap();
    assert_eq!((parsed.application.as_str(), parsed.secret.as_str()), ("ak", "as"));
    assert_eq!(parsed.consumer, "ck");
    for password in ["sans-deux-points", ":ck", "as:"] {
        let bad = Credential::UsernamePassword { username: "ak".into(), password: password.into() };
        assert!(matches!(keys(&bad), Err(ProbeError::Config(_))), "{password}");
    }
    assert!(matches!(
        keys(&Credential::ApiToken { token: "x".into() }),
        Err(ProbeError::Config(_))
    ));
}

#[tokio::test]
async fn serveurs_vps_et_dedies() {
    let address = serveur("ok").await;
    let s = OvhCollector::new().probe(&cible_ovh(&address, "secret:consumer")).await.unwrap();
    assert_eq!(value(&s, "ovh_vps_up", &[("vps", "vps-aaa111.vps.ovh.net")]), 1.0);
    assert_eq!(value(&s, "ovh_vps_up", &[("vps", "vps-bbb222.vps.ovh.net")]), 0.0);
    let info = find(&s, "ovh_vps_info", &[("vps", "vps-aaa111.vps.ovh.net")]).unwrap();
    assert_eq!(info.labels["state"], "running");
    assert_eq!(info.labels["model"], "vps-ssd-2");
    assert_eq!(value(&s, "ovh_dedicated_up", &[("server", "ns100.ip-1-2-3.eu")]), 1.0);
    assert_eq!(value(&s, "ovh_dedicated_up", &[("server", "ns200.ip-1-2-4.eu")]), 0.0);
    assert_eq!(value(&s, "ovh_dedicated_monitoring", &[("server", "ns100.ip-1-2-3.eu")]), 1.0);
    assert_eq!(value(&s, "ovh_vps_tasks_active", &[("vps", "vps-bbb222.vps.ovh.net")]), 1.0);
    assert_eq!(value(&s, "ovh_dedicated_tasks_active", &[("server", "ns100.ip-1-2-3.eu")]), 2.0);
    assert_eq!(value(&s, "ovh_servers", &[]), 4.0);
    assert_eq!(value(&s, "ovh_servers_down", &[]), 2.0);
    assert_eq!(value(&s, "ovh_tasks_active", &[]), 3.0);
    for readable in ["ovh_vps_readable", "ovh_dedicated_readable", "ovh_tasks_readable"] {
        assert_eq!(value(&s, readable, &[]), 1.0, "{readable}");
    }
    let item = find(&s, "ovh_item_ok", &[("group", "VPS"), ("item", "db-1")]).unwrap();
    assert_eq!(item.value, 0.0);
    assert_eq!(item.labels["detail"], "stopped");
    assert_eq!(value(&s, "ovh_figure", &[("name", "Servers down")]), 2.0);
}

#[test]
fn l_echeance_du_service_en_secondes() {
    // 1_700_000_000 = 2023-11-14T22:13:20Z ; minuit UTC du 15 : 6400 s plus tard.
    assert_eq!(seconds_until("2023-11-15", 1_700_000_000_000), Some(6400.0));
    assert_eq!(seconds_until("2023-11-14T00:00:00Z", 1_700_000_000_000), Some(-80_000.0));
    assert_eq!(seconds_until("pas une date", 0), None);
    let server = Server {
        name: "vps-x".into(),
        detail: Value::Null,
        tasks: None,
        expiration: Some("2023-11-15".into()),
    };
    let sample = expiry(&server, "vps", 1_700_000_000_000).unwrap();
    assert_eq!(sample.metric, "ovh_service_expiry_seconds");
    assert_eq!(sample.value, 6400.0);
    assert_eq!(sample.labels["service"], "vps-x");
    let none = Server { expiration: None, ..server };
    assert!(expiry(&none, "vps", 0).is_none());
}

#[tokio::test]
async fn les_echeances_sont_lues_et_une_lecture_refusee_est_ignoree() {
    let address = serveur("ok").await;
    let s = OvhCollector::new().probe(&cible_ovh(&address, "secret:consumer")).await.unwrap();
    let found =
        find(&s, "ovh_service_expiry_seconds", &[("service", "vps-aaa111.vps.ovh.net")]).unwrap();
    assert!(found.value > 0.0);
    assert_eq!(found.labels["family"], "vps");
    // Les autres serveurs n'ont pas de serviceInfos (404) : pas de série, pas d'échec.
    assert!(find(&s, "ovh_service_expiry_seconds", &[("service", "ns100.ip-1-2-3.eu")]).is_none());
}

#[tokio::test]
async fn une_famille_refusee_n_echoue_pas() {
    let address = serveur("dedicated_denied").await;
    let s = OvhCollector::new().probe(&cible_ovh(&address, "secret:consumer")).await.unwrap();
    assert_eq!(value(&s, "ovh_dedicated_readable", &[]), 0.0);
    assert_eq!(value(&s, "ovh_vps_readable", &[]), 1.0);
    assert!(find(&s, "ovh_dedicated_up", &[]).is_none());
    assert_eq!(value(&s, "ovh_servers", &[]), 2.0);
}

#[tokio::test]
async fn des_taches_illisibles_n_echouent_pas() {
    let address = serveur("tasks_denied").await;
    let s = OvhCollector::new().probe(&cible_ovh(&address, "secret:consumer")).await.unwrap();
    assert_eq!(value(&s, "ovh_tasks_readable", &[]), 0.0);
    assert!(find(&s, "ovh_vps_tasks_active", &[]).is_none());
    assert_eq!(value(&s, "ovh_vps_up", &[("vps", "vps-aaa111.vps.ovh.net")]), 1.0);
}

#[tokio::test]
async fn identifiants_refuses() {
    let collector = OvhCollector::new();
    // Aucun droit sur aucune famille.
    let address = serveur("all_denied").await;
    let error = collector.probe(&cible_ovh(&address, "secret:consumer")).await.unwrap_err();
    assert!(matches!(error, ProbeError::Auth(_)), "{error}");
    // Clé de consommateur révoquée ou expirée.
    let address = serveur("revoked").await;
    let error = collector.probe(&cible_ovh(&address, "secret:consumer")).await.unwrap_err();
    assert!(matches!(error, ProbeError::Auth(_)), "{error}");
    // Mauvais secret : la signature ne correspond pas.
    let address = serveur("ok").await;
    let error = collector.probe(&cible_ovh(&address, "wrong:consumer")).await.unwrap_err();
    assert!(matches!(error, ProbeError::Auth(_)), "{error}");
    assert!(!error.means_down());
}

#[tokio::test]
async fn un_service_en_panne_est_une_panne() {
    let address = serveur("down").await;
    let target = cible_ovh(&address, "secret:consumer");
    let error = OvhCollector::new().probe(&target).await.unwrap_err();
    assert!(error.means_down(), "{error}");
}
