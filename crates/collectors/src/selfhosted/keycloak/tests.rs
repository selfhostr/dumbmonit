use std::collections::BTreeMap;
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};

use super::*;

// Réponses d'un Keycloak 26.0.5 : santé de l'interface de gestion (prête,
// puis base de données tombée), `serverinfo`, trois domaines dont un
// désactivé, statistiques de sessions du domaine « homelab », et le refus
// d'un client dont le secret est faux.
const READY: &str = include_str!("../testdata/keycloak_26.0.5/health_ready.json");
const READY_DOWN: &str = include_str!("../testdata/keycloak_26.0.5/health_ready_down.json");
const LIVE: &str = include_str!("../testdata/keycloak_26.0.5/health_live.json");
const SERVERINFO: &str = include_str!("../testdata/keycloak_26.0.5/serverinfo.json");
const REALMS: &str = include_str!("../testdata/keycloak_26.0.5/realms.json");
const SESSIONS: &str =
    include_str!("../testdata/keycloak_26.0.5/client_session_stats_homelab.json");
const TOKEN: &str = include_str!("../testdata/keycloak_26.0.5/token.json");
const TOKEN_REFUSED: &str = include_str!("../testdata/keycloak_26.0.5/token_refused.json");

// Keycloak 26.8.2, d'après le relevé d'une instance réelle : les domaines
// `master`, `labrealm` (3 utilisateurs, 6 sessions) et `lockedrealm`
// (désactivé), les statistiques de sessions de `labrealm` vues avec les
// bons rôles, puis `[]` (200) pour le même domaine sans `view-clients`.
// `admin_unauthorized.json` : le corps du 401 de l'API d'administration
// quand le jeton n'a pas l'audience `kc-admin-api` (le motif n'est écrit
// que dans le journal de Keycloak). Les listes de clients suivent
// `ClientRepresentation` de l'API d'administration (champ `access` posé
// par `ClientsResource.getClients`) : un client visible, puis la liste
// vide qu'obtient un compte qui peut lister sans voir. `serverinfo_limited`
// : la page sans `systemInfo` ni `memoryInfo`, servie sans `manage-realm`.
const REALMS_26: &str = include_str!("../testdata/keycloak_26.8.2/realms.json");
const SESSIONS_LAB: &str =
    include_str!("../testdata/keycloak_26.8.2/client_session_stats_labrealm.json");
const SESSIONS_HIDDEN: &str =
    include_str!("../testdata/keycloak_26.8.2/client_session_stats_hidden.json");
const CLIENTS_VIEWABLE: &str = include_str!("../testdata/keycloak_26.8.2/clients_viewable.json");
const CLIENTS_HIDDEN: &str = include_str!("../testdata/keycloak_26.8.2/clients_hidden.json");
const ADMIN_UNAUTHORIZED: &str =
    include_str!("../testdata/keycloak_26.8.2/admin_unauthorized.json");
const SERVERINFO_LIMITED: &str =
    include_str!("../testdata/keycloak_26.8.2/serverinfo_limited.json");

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn value(samples: &[Sample], name: &str) -> Option<f64> {
    samples.iter().find(|s| s.metric == name).map(|s| s.value)
}

fn realm(samples: &[Sample], name: &str, realm: &str) -> Option<f64> {
    samples
        .iter()
        .find(|s| s.metric == name && s.labels.get("realm").map(String::as_str) == Some(realm))
        .map(|s| s.value)
}

#[test]
fn la_base_de_donnees_tombee_se_voit_dans_les_verifications() {
    let samples = health_samples("ready", &json(READY_DOWN), 0);
    assert_eq!(value(&samples, "keycloak_ready"), Some(0.0));
    let check = samples.iter().find(|s| s.metric == "keycloak_health_check_up").unwrap();
    assert_eq!(check.value, 0.0);
    assert!(check.labels["check"].contains("database"));
    let samples = health_samples("ready", &json(READY), 0);
    assert_eq!(value(&samples, "keycloak_ready"), Some(1.0));
    let live = health_samples("live", &json(LIVE), 0);
    assert_eq!(live.len(), 1);
    assert_eq!(value(&live, "keycloak_live"), Some(1.0));
}

#[test]
fn serverinfo_donne_version_duree_de_vie_et_memoire() {
    let samples = serverinfo_samples(&json(SERVERINFO), 0);
    let info = samples.iter().find(|s| s.metric == "keycloak_version_info").unwrap();
    assert_eq!(info.labels["version"], "26.0.5");
    assert_eq!(value(&samples, "keycloak_uptime_seconds"), Some(443_528.0));
    assert_eq!(value(&samples, "keycloak_memory_used_percent"), Some(75.0));
}

#[test]
fn les_domaines_se_comptent_et_se_filtrent() {
    let (samples, names) = realm_samples(&json(REALMS), &[], 0);
    assert_eq!(value(&samples, "keycloak_realms_total"), Some(3.0));
    assert_eq!(value(&samples, "keycloak_realms_disabled"), Some(1.0));
    assert_eq!(names, ["master", "homelab", "old test"]);
    let (_, names) = realm_samples(&json(REALMS), &["homelab".to_string()], 0);
    assert_eq!(names, ["homelab"]);
}

#[test]
fn les_sessions_arrivent_en_texte_et_se_somment() {
    let counts = session_counts(&json(SESSIONS));
    assert_eq!(counts, (13.0, 1.0));
    let samples = realm_detail_samples("homelab", Some(counts), Some(42.0), 0);
    assert_eq!(realm(&samples, "keycloak_realm_sessions", "homelab"), Some(13.0));
    assert_eq!(realm(&samples, "keycloak_realm_offline_sessions", "homelab"), Some(1.0));
    assert_eq!(realm(&samples, "keycloak_realm_users", "homelab"), Some(42.0));
    assert!(realm_detail_samples("x", None, None, 0).is_empty());
    assert_eq!(session_counts(&json(SESSIONS_LAB)), (6.0, 0.0));
    assert_eq!(session_counts(&json(SESSIONS_HIDDEN)), (0.0, 0.0));
}

#[test]
fn des_clients_caches_rendent_les_sessions_illisibles() {
    assert!(clients_viewable(Some(&json(CLIENTS_VIEWABLE))));
    assert!(!clients_viewable(Some(&json(CLIENTS_HIDDEN))));
    assert!(!clients_viewable(None), "403 on the client list");
    let refused = json(r#"[{"clientId":"account","access":{"view":false}}]"#);
    assert!(!clients_viewable(Some(&refused)));
}

#[test]
fn sans_manage_realm_serverinfo_ne_donne_ni_version_ni_memoire() {
    assert!(serverinfo_samples(&json(SERVERINFO_LIMITED), 0).is_empty());
}

#[test]
fn l_adresse_de_gestion_garde_l_hote_seulement() {
    assert_eq!(host_only("sso.lan"), "sso.lan");
    assert_eq!(host_only("sso.lan:8080"), "sso.lan");
    assert_eq!(host_only("https://sso.example.com/auth"), "sso.example.com");
    assert_eq!(host_only("[fd00::1]:8443"), "[fd00::1]");
    assert_eq!(host_only("fd00::1"), "fd00::1");
    assert_eq!(encode("old test"), "old%20test");
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}

fn target(address: String, credential: Credential, tags: &[(&str, &str)]) -> Target {
    Target {
        id: 6,
        name: "sso".into(),
        address,
        kind: "keycloak".into(),
        profile_id: None,
        parent_id: None,
        interval: Duration::from_secs(60),
        enabled: true,
        tags: tags.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
        credential,
        group_name: String::new(),
        position: 0,
    }
}

fn secret(password: &str) -> Credential {
    Credential::UsernamePassword { username: "dumbmonit".into(), password: password.into() }
}

fn app() -> Router {
    Router::new()
        .route("/health/ready", get(|| async { READY }))
        .route("/health/live", get(|| async { LIVE }))
        .route(
            "/realms/master/protocol/openid-connect/token",
            post(|body: String| async move {
                if body.contains("client_secret=bon") {
                    (StatusCode::OK, TOKEN)
                } else {
                    (StatusCode::UNAUTHORIZED, TOKEN_REFUSED)
                }
            }),
        )
        .route(
            "/admin/serverinfo",
            get(|headers: HeaderMap| async move {
                let bearer = headers.get("authorization").and_then(|v| v.to_str().ok());
                if bearer.is_some_and(|b| b.starts_with("Bearer eyJ")) {
                    (StatusCode::OK, SERVERINFO)
                } else {
                    (StatusCode::UNAUTHORIZED, "{}")
                }
            }),
        )
        .route("/admin/realms", get(|| async { REALMS }))
        .route("/admin/realms/homelab/client-session-stats", get(|| async { SESSIONS }))
        .route("/admin/realms/homelab/users/count", get(|| async { "42" }))
        // Les autres domaines : le compte de service n'a pas le droit d'y lire.
        .fallback(|| async { (StatusCode::FORBIDDEN, r#"{"error":"HTTP 403 Forbidden"}"#) })
}

#[tokio::test]
async fn la_sante_seule_se_lit_sans_identifiants() {
    let base = serve(app()).await;
    let tags = [("health_port", "same")];
    let samples =
        KeycloakCollector::new().probe(&target(base, Credential::None, &tags)).await.unwrap();
    assert_eq!(value(&samples, "keycloak_ready"), Some(1.0));
    assert_eq!(value(&samples, "keycloak_live"), Some(1.0));
    assert!(value(&samples, "keycloak_admin_readable").is_none());
    assert_eq!(value(&samples, "keycloak_scrape_errors"), Some(0.0));
}

#[tokio::test]
async fn l_administration_donne_domaines_sessions_et_utilisateurs() {
    let base = serve(app()).await;
    let tags = [("health_port", "same"), ("realms", "homelab, master")];
    let samples =
        KeycloakCollector::new().probe(&target(base, secret("bon"), &tags)).await.unwrap();
    assert_eq!(value(&samples, "keycloak_admin_readable"), Some(1.0));
    assert_eq!(value(&samples, "keycloak_realms_total"), Some(3.0));
    assert_eq!(realm(&samples, "keycloak_realm_sessions", "homelab"), Some(13.0));
    assert_eq!(realm(&samples, "keycloak_realm_users", "homelab"), Some(42.0));
    // « master » est interdit au compte de service : sauté, jamais une panne.
    assert!(realm(&samples, "keycloak_realm_sessions", "master").is_none());
    assert_eq!(value(&samples, "keycloak_sessions_active"), Some(13.0));
    assert_eq!(value(&samples, "keycloak_uptime_seconds"), Some(443_528.0));
}

#[tokio::test]
async fn un_secret_refuse_est_une_erreur_d_authentification() {
    let base = serve(app()).await;
    let tags = [("health_port", "same")];
    let error =
        KeycloakCollector::new().probe(&target(base, secret("faux"), &tags)).await.unwrap_err();
    assert!(matches!(error, ProbeError::Auth(_)));
    assert!(!error.means_down());
}

/// Un Keycloak 26.8.2 : le jeton est délivré, puis l'API d'administration
/// le refuse (401) faute de client scope `kc-admin-api`.
fn app_audience_refusee() -> Router {
    Router::new()
        .route("/health/ready", get(|| async { READY }))
        .route("/health/live", get(|| async { LIVE }))
        .route("/realms/master/protocol/openid-connect/token", post(|| async { TOKEN }))
        .fallback(|| async { (StatusCode::UNAUTHORIZED, ADMIN_UNAUTHORIZED) })
}

#[tokio::test]
async fn un_jeton_refuse_par_l_administration_n_est_pas_lisible() {
    let base = serve(app_audience_refusee()).await;
    let tags = [("health_port", "same")];
    let target = target(base, secret("bon"), &tags);
    let samples = KeycloakCollector::new().probe(&target).await.unwrap();
    assert_eq!(value(&samples, "keycloak_ready"), Some(1.0));
    assert_eq!(value(&samples, "keycloak_admin_readable"), Some(0.0));
    assert!(value(&samples, "keycloak_realms_total").is_none());
    assert!(value(&samples, "keycloak_uptime_seconds").is_none());
    // `/admin/realms` seul : `serverinfo` n'est pas tenté après le refus.
    assert_eq!(value(&samples, "keycloak_scrape_errors"), Some(1.0));

    let admin = http_client(&target, "http", DEFAULT_PORT, Auth::default(), PRODUCT).unwrap();
    let error = admin_get(&admin, "/admin/realms").await.unwrap_err();
    assert!(matches!(error, ProbeError::Auth(ref m) if m.contains("kc-admin-api")), "{error}");
}

/// Les domaines du relevé : `master` sans session (clients visibles),
/// `labrealm` avec ou sans le rôle `view-clients`.
fn app_sessions(view_clients: bool) -> Router {
    let (stats, clients) = if view_clients {
        (SESSIONS_LAB, CLIENTS_VIEWABLE)
    } else {
        (SESSIONS_HIDDEN, CLIENTS_HIDDEN)
    };
    Router::new()
        .route("/health/ready", get(|| async { READY }))
        .route("/health/live", get(|| async { LIVE }))
        .route("/realms/master/protocol/openid-connect/token", post(|| async { TOKEN }))
        .route("/admin/serverinfo", get(|| async { SERVERINFO_LIMITED }))
        .route("/admin/realms", get(|| async { REALMS_26 }))
        .route("/admin/realms/master/client-session-stats", get(|| async { SESSIONS_HIDDEN }))
        .route("/admin/realms/master/clients", get(|| async { CLIENTS_VIEWABLE }))
        .route("/admin/realms/labrealm/client-session-stats", get(move || async move { stats }))
        .route("/admin/realms/labrealm/clients", get(move || async move { clients }))
        .route("/admin/realms/labrealm/users/count", get(|| async { "3" }))
        .fallback(|| async { (StatusCode::FORBIDDEN, r#"{"error":"HTTP 403 Forbidden"}"#) })
}

#[tokio::test]
async fn les_sessions_cachees_ne_valent_pas_zero() {
    let tags = [("health_port", "same")];
    let base = serve(app_sessions(true)).await;
    let samples =
        KeycloakCollector::new().probe(&target(base, secret("bon"), &tags)).await.unwrap();
    assert_eq!(value(&samples, "keycloak_admin_readable"), Some(1.0));
    assert_eq!(value(&samples, "keycloak_realms_total"), Some(3.0));
    assert_eq!(value(&samples, "keycloak_realms_disabled"), Some(1.0));
    assert_eq!(realm(&samples, "keycloak_realm_sessions", "labrealm"), Some(6.0));
    assert_eq!(realm(&samples, "keycloak_realm_users", "labrealm"), Some(3.0));
    // Aucune session dans `master`, et des clients visibles : un vrai zéro.
    assert_eq!(realm(&samples, "keycloak_realm_sessions", "master"), Some(0.0));
    assert_eq!(value(&samples, "keycloak_sessions_active"), Some(6.0));
    // Sans `manage-realm`, ni version ni durée de vie : rien n'est inventé.
    assert!(value(&samples, "keycloak_version_info").is_none());
    assert_eq!(value(&samples, "keycloak_scrape_errors"), Some(0.0));

    let base = serve(app_sessions(false)).await;
    let samples =
        KeycloakCollector::new().probe(&target(base, secret("bon"), &tags)).await.unwrap();
    assert!(realm(&samples, "keycloak_realm_sessions", "labrealm").is_none());
    assert_eq!(realm(&samples, "keycloak_realm_users", "labrealm"), Some(3.0));
    assert_eq!(realm(&samples, "keycloak_realm_sessions", "master"), Some(0.0));
    assert_eq!(value(&samples, "keycloak_scrape_errors"), Some(1.0));
}

#[tokio::test]
async fn la_sante_en_echec_reste_une_mesure_et_l_injoignable_une_panne() {
    let app = Router::new()
        .route("/health/ready", get(|| async { (StatusCode::SERVICE_UNAVAILABLE, READY_DOWN) }))
        .route("/health/live", get(|| async { LIVE }));
    let base = serve(app).await;
    let tags = [("health_port", "same")];
    let samples =
        KeycloakCollector::new().probe(&target(base, Credential::None, &tags)).await.unwrap();
    assert_eq!(value(&samples, "keycloak_ready"), Some(0.0));

    let error = KeycloakCollector::new()
        .probe(&target("127.0.0.1".into(), Credential::None, &[("health_port", "1")]))
        .await
        .unwrap_err();
    assert!(error.means_down());
    let error = KeycloakCollector::new()
        .probe(&target("127.0.0.1".into(), Credential::None, &[("health_port", "neuf")]))
        .await
        .unwrap_err();
    assert!(matches!(error, ProbeError::Config(_)));
}
