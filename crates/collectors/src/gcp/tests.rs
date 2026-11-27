use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};

use super::*;
use crate::rest::test_support::{find, serve, value};
use crate::uptime::tags::test_support::cible;

/// Clé jetable, générée pour les tests : elle ne donne accès à rien.
const TEST_KEY: &str = include_str!("testdata/test_key.pem");

fn key_json() -> String {
    serde_json::json!({
        "type": "service_account",
        "project_id": "demo-project",
        "client_email": "dumbmonit@demo-project.iam.gserviceaccount.com",
        "private_key": TEST_KEY,
    })
    .to_string()
}

#[test]
fn jwt_signe_rs256_verifiable() {
    let key: ServiceKey = serde_json::from_str(&key_json()).unwrap();
    let jwt = signed_jwt(&key, "https://oauth2.googleapis.com/token", 1_000).unwrap();
    let parts: Vec<&str> = jwt.split('.').collect();
    assert_eq!(parts.len(), 3);
    let claims = URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
    let claims: Value = serde_json::from_slice(&claims).unwrap();
    assert_eq!(claims["iss"], key.client_email);
    assert_eq!(claims["exp"], 4_600);
    let pem: String = key.private_key.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = STANDARD.decode(pem).unwrap();
    let pair = ring::signature::RsaKeyPair::from_pkcs8(&der).unwrap();
    let public = ring::signature::UnparsedPublicKey::new(
        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        pair.public().as_ref(),
    );
    let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
    public.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature).unwrap();
}

#[test]
fn cle_invalide_est_une_erreur_de_configuration() {
    let mut target = cible("gcp", "googleapis.com", &[]);
    target.credential = Credential::ApiToken { token: "pas du json".into() };
    assert!(matches!(service_key(&target), Err(ProbeError::Config(_))));
    target.credential = Credential::None;
    assert!(matches!(service_key(&target), Err(ProbeError::Config(_))));
}

#[test]
fn classement_des_etats() {
    assert_eq!(classify_compute("RUNNING"), Health::Running);
    assert_eq!(classify_compute("TERMINATED"), Health::Stopped);
    assert_eq!(classify_compute("REPAIRING"), Health::Failed);
    assert_eq!(classify_compute("STAGING"), Health::Transition);
    assert_eq!(classify_sql("RUNNABLE"), Health::Running);
    assert_eq!(classify_sql("SUSPENDED"), Health::Stopped);
    assert_eq!(classify_sql("FAILED"), Health::Failed);
}

#[test]
fn encodage_de_requete() {
    assert_eq!(encode("metric.type=\"a/b\""), "metric.type%3D%22a%2Fb%22");
}

struct Fake {
    logins: AtomicUsize,
    sql_status: u16,
    monitoring_status: u16,
}

impl Fake {
    fn new(sql_status: u16, monitoring_status: u16) -> Self {
        Self { logins: AtomicUsize::new(0), sql_status, monitoring_status }
    }
}

async fn fake(state: Arc<Fake>) -> String {
    async fn token(State(fake): State<Arc<Fake>>, body: String) -> (StatusCode, String) {
        assert!(body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer"));
        assert!(body.contains("assertion=ey"), "{body}");
        fake.logins.fetch_add(1, Ordering::SeqCst);
        (StatusCode::OK, r#"{"access_token":"tok-gcp","expires_in":3599}"#.into())
    }
    fn authorized(headers: &HeaderMap) -> bool {
        headers.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer tok-gcp")
    }
    async fn compute(headers: HeaderMap, RawQuery(query): RawQuery) -> (StatusCode, String) {
        if !authorized(&headers) {
            return (StatusCode::UNAUTHORIZED, "{}".into());
        }
        let page = if query.unwrap_or_default().contains("pageToken=p2") {
            r#"{"items":{"zones/europe-west1-c":{"instances":[
                {"name":"vm3","status":"TERMINATED","zone":"https://x/zones/europe-west1-c"}]}}}"#
        } else {
            r#"{"nextPageToken":"p2","items":{"zones/europe-west1-b":{"instances":[
                {"name":"vm1","status":"RUNNING","zone":"https://x/zones/europe-west1-b"},
                {"name":"vm2","status":"REPAIRING","zone":"https://x/zones/europe-west1-b"}]},
              "zones/europe-west1-d":{"warning":{"code":"NO_RESULTS_ON_PAGE"}}}}"#
        };
        (StatusCode::OK, page.into())
    }
    async fn sql(State(fake): State<Arc<Fake>>) -> (StatusCode, String) {
        if fake.sql_status != 200 {
            return (StatusCode::from_u16(fake.sql_status).unwrap(), "{}".into());
        }
        let body = r#"{"items":[{"name":"db1","state":"RUNNABLE"},
            {"name":"db2","state":"FAILED"}]}"#;
        (StatusCode::OK, body.into())
    }
    async fn series(
        State(fake): State<Arc<Fake>>,
        RawQuery(query): RawQuery,
    ) -> (StatusCode, String) {
        if fake.monitoring_status != 200 {
            return (StatusCode::from_u16(fake.monitoring_status).unwrap(), "{}".into());
        }
        let query = query.unwrap_or_default();
        assert!(query.contains("cpu%2Futilization"), "{query}");
        let body = r#"{"timeSeries":[{"metric":{"labels":{"instance_name":"vm1"}},
            "resource":{"labels":{"instance_id":"123"}},
            "points":[{"value":{"doubleValue":0.42}},{"value":{"doubleValue":0.1}}]}]}"#;
        (StatusCode::OK, body.into())
    }
    let app = Router::new()
        .route("/token", post(token))
        .route("/compute/v1/projects/{p}/aggregated/instances", get(compute))
        .route("/sql/v1beta4/projects/{p}/instances", get(sql))
        .route("/v3/projects/{p}/timeSeries", get(series))
        .with_state(state);
    serve(app).await
}

fn cible_gcp(address: &str) -> Target {
    let url = format!("http://{address}");
    let token = format!("{url}/token");
    let tags = [("api_url", url.as_str()), ("token_url", token.as_str())];
    let mut target = cible("gcp", &url, &tags);
    target.credential = Credential::ApiToken { token: key_json() };
    target
}

#[tokio::test]
async fn sonde_complete_pagination_et_jeton_garde() {
    let state = Arc::new(Fake::new(200, 200));
    let address = fake(state.clone()).await;
    let collector = GcpCollector::new();
    let target = cible_gcp(&address);
    let s = collector.probe(&target).await.unwrap();
    assert_eq!(value(&s, "gcp_instances", &[]), 3.0, "both pages are read");
    assert_eq!(value(&s, "gcp_instances_running", &[]), 1.0);
    assert_eq!(value(&s, "gcp_instances_stopped", &[]), 1.0);
    assert_eq!(value(&s, "gcp_instances_failed", &[]), 1.0);
    let labels = [("instance", "vm2"), ("zone", "europe-west1-b")];
    assert!(find(&s, "gcp_instance_unhealthy", &labels).is_some());
    assert_eq!(value(&s, "gcp_sql_instances_failed", &[]), 1.0);
    assert_eq!(value(&s, "gcp_monitoring_readable", &[]), 1.0);
    let cpu = value(&s, "gcp_metric", &[("resource", "vm1"), ("unit", "%")]);
    assert!((cpu - 42.0).abs() < 1e-9, "latest point, as a percentage");
    collector.probe(&target).await.unwrap();
    assert_eq!(state.logins.load(Ordering::SeqCst), 1, "the token is kept");
    assert_eq!(collector.discover(&target).await.unwrap().as_deref(), Some("gcp"));
}

#[tokio::test]
async fn sql_et_monitoring_refuses_sans_echec() {
    let state = Arc::new(Fake::new(403, 403));
    let address = fake(state).await;
    let s = GcpCollector::new().probe(&cible_gcp(&address)).await.unwrap();
    assert_eq!(value(&s, "gcp_sql_readable", &[]), 0.0);
    assert_eq!(value(&s, "gcp_monitoring_readable", &[]), 0.0);
    assert_eq!(value(&s, "gcp_instances", &[]), 3.0);
}
