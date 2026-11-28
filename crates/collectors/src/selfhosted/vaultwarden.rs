//! Vaultwarden.
//!
//! Deux niveaux, le second facultatif.
//!
//! 1. **Public** : `/alive` (la preuve de vie, que Vaultwarden sert sans
//!    identifiant), `/api/version` (la version de Vaultwarden) et `/api/config`
//!    (l'état des inscriptions ; sa propre `version` est celle de compatibilité
//!    Bitwarden, pas celle de Vaultwarden). Aucun secret, aucun jeton :
//!    c'est tout ce qu'il faut pour savoir que le coffre répond.
//! 2. **Administration** : avec le jeton d'administration (`ADMIN_TOKEN`), le
//!    nombre d'utilisateurs, de comptes désactivés, de comptes sans second
//!    facteur et d'éléments. Vaultwarden n'a pas d'API d'administration à
//!    jeton : le jeton ouvre une session sur la page `/admin` (cookie
//!    `VW_ADMIN`, vingt minutes). Cette ouverture est limitée (trois essais,
//!    puis un toutes les cinq minutes) : le cookie est donc gardé et réutilisé
//!    entre deux sondes, jamais renégocié à chaque minute, mais oublié dès que
//!    le jeton change ou que Vaultwarden le refuse (401, 403, retour à la page
//!    de connexion). Le nombre d'éléments n'est plus donné par Vaultwarden
//!    1.37 : il n'est publié que si la liste le fournit.
//!
//! Aucun courriel, nom, mot de passe ou élément de coffre n'est lu ou publié :
//! seuls des compteurs sortent de la page des utilisateurs.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dumbmonit_proto::{Collector, ProbeError, Sample, Target, TargetId};
use reqwest::StatusCode;
use serde_json::Value;

use super::client::{Auth, HttpClient, cookies_of};
use super::options::Options;
use super::{flag, gauge, http_client, metric, now_ms, number, settle, token};

pub const DEFAULT_PORT: u16 = 443;
const PRODUCT: &str = "Vaultwarden";
const P: &str = "vaultwarden";

/// Durée pendant laquelle un cookie d'administration est réutilisé. La
/// session de Vaultwarden dure vingt minutes par défaut.
const SESSION_REUSE: Duration = Duration::from_secs(15 * 60);

// ------------------------------------------------------------ traduction

/// `GET /api/version` : la version de Vaultwarden, une chaîne JSON (`"1.37.5"`).
pub fn server_version(body: &str) -> Option<String> {
    let version = serde_json::from_str::<String>(body.trim())
        .unwrap_or_else(|_| body.trim().trim_matches('"').to_string());
    (!version.is_empty()).then_some(version)
}

/// `GET /api/config`. `/api/config` donne la version de *compatibilité
/// Bitwarden* (`2026.6.0`), pas celle de Vaultwarden : on lui préfère
/// `server_version` (`/api/version`) et on ne retombe sur elle que faute de mieux.
pub fn config_samples(body: &Value, server_version: Option<&str>, ts_ms: i64) -> Vec<Sample> {
    let version =
        server_version.or_else(|| body.get("version").and_then(Value::as_str)).unwrap_or("unknown");
    let mut out =
        vec![gauge(&metric(P, "version_info"), 1.0, ts_ms).with_label("version", version)];
    if let Some(closed) = body.pointer("/settings/disableUserRegistration").and_then(Value::as_bool)
    {
        out.push(flag(&metric(P, "signups_open"), !closed, ts_ms));
    }
    out
}

/// La première des clés présente, lue comme booléen. Vaultwarden a changé la
/// casse des champs de `/admin/users` d'une version à l'autre : `user_enabled`
/// et `TwoFactorEnabled` (≤ 1.34), `userEnabled` et `twoFactorEnabled` (1.37).
fn bool_field(user: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| user.get(*key).and_then(Value::as_bool))
}

/// `GET /admin/users` : uniquement des compteurs.
pub fn user_samples(body: &Value, ts_ms: i64) -> Vec<Sample> {
    let users = body.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let enabled = |user: &Value| bool_field(user, &["userEnabled", "user_enabled"]).unwrap_or(true);
    let two_factor = |user: &Value| {
        bool_field(user, &["twoFactorEnabled", "TwoFactorEnabled", "two_factor_enabled"])
            .unwrap_or(false)
    };
    let disabled = users.iter().filter(|u| !enabled(u)).count();
    // Un compte désactivé n'a pas à avoir de second facteur : on ne compte que
    // les comptes actifs qui s'en passent.
    let without_2fa = users.iter().filter(|u| enabled(u) && !two_factor(u)).count();
    // Le nombre d'éléments a disparu de la liste (1.37) : sans le champ nulle
    // part, on ne publie rien plutôt qu'un zéro faux.
    let ciphers: Vec<f64> = users
        .iter()
        .filter_map(|u| number(u.get("cipher_count").or_else(|| u.get("cipherCount"))))
        .collect();
    let mut out = vec![
        flag(&metric(P, "admin_readable"), true, ts_ms),
        gauge(&metric(P, "users"), users.len() as f64, ts_ms),
        gauge(&metric(P, "users_disabled"), disabled as f64, ts_ms),
        gauge(&metric(P, "users_without_2fa"), without_2fa as f64, ts_ms),
    ];
    if !ciphers.is_empty() {
        out.push(gauge(&metric(P, "items"), ciphers.iter().sum(), ts_ms));
    }
    out
}

// ------------------------------------------------------------------ sonde

fn net(error: &reqwest::Error, timeout: Duration, path: &str) -> ProbeError {
    if error.is_timeout() {
        return ProbeError::Timeout(timeout);
    }
    ProbeError::Unreachable(format!("{path}: {error}"))
}

/// Un client qui ne suit aucune redirection : la connexion à `/admin` répond
/// par un `303` dont c'est précisément le cookie qu'on veut lire.
fn admin_http(options: &Options) -> Result<reqwest::Client, ProbeError> {
    crate::http::tls_policy(reqwest::Client::builder(), options.insecure_tls)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("DumbMonit/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| ProbeError::Config(format!("HTTP client unavailable: {error}")))
}

async fn admin_login(
    http: &reqwest::Client,
    options: &Options,
    admin_token: &str,
) -> Result<String, ProbeError> {
    let path = "/admin";
    let response = http
        .post(format!("{}{path}", options.base_url))
        .form(&[("token", admin_token)])
        .timeout(options.request_timeout)
        .send()
        .await
        .map_err(|error| net(&error, options.request_timeout, path))?;
    let status = response.status();
    let cookie = cookies_of(response.headers());
    match status {
        StatusCode::NOT_FOUND => Err(ProbeError::Protocol(
            "Vaultwarden's admin page is disabled (no ADMIN_TOKEN set on the server).".to_string(),
        )),
        StatusCode::TOO_MANY_REQUESTS => Err(ProbeError::Protocol(
            "Vaultwarden rate-limited the admin login: probe less often, or raise \
             ADMIN_RATELIMIT_MAX_BURST."
                .to_string(),
        )),
        _ if cookie.is_empty() => Err(ProbeError::Auth(
            "Vaultwarden refused the admin token. Use the plain token typed on the /admin \
             page, not its hash."
                .to_string(),
        )),
        _ => Ok(cookie),
    }
}

/// La liste des utilisateurs, ou `None` si la session n'est plus valable.
async fn fetch_users(
    http: &reqwest::Client,
    options: &Options,
    cookie: &str,
) -> Result<Option<Value>, ProbeError> {
    let path = "/admin/users";
    let response = http
        .get(format!("{}{path}", options.base_url))
        .header(reqwest::header::COOKIE, cookie)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(options.request_timeout)
        .send()
        .await
        .map_err(|error| net(&error, options.request_timeout, path))?;
    let status = response.status();
    if status.is_redirection() || matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
    {
        return Ok(None);
    }
    let body = response.text().await.map_err(|error| net(&error, options.request_timeout, path))?;
    if !status.is_success() {
        let excerpt: String = body.chars().take(200).collect();
        return Err(if status.is_server_error() {
            ProbeError::Unreachable(format!("{PRODUCT} answered {status} on {path}: {excerpt}"))
        } else {
            ProbeError::Protocol(format!("{PRODUCT} answered {status} on {path}: {excerpt}"))
        });
    }
    serde_json::from_str(&body).map(Some).map_err(|error| {
        ProbeError::Protocol(format!("{PRODUCT} sent an unexpected answer on {path}: {error}"))
    })
}

/// `GET /api/version`, public : la version de Vaultwarden.
async fn fetch_version(client: &HttpClient) -> Result<Option<String>, ProbeError> {
    let path = "/api/version";
    let (status, body) = client.get(path).await?;
    if !status.is_success() {
        return Err(client.status_error(status, &body, path));
    }
    Ok(server_version(&body))
}

// ------------------------------------------------------------ collecteur

/// Une empreinte du jeton, pour reconnaître qu'il a changé sans garder le jeton.
fn fingerprint(admin_token: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    admin_token.hash(&mut hasher);
    hasher.finish()
}

/// Le cookie d'administration d'une cible : sa valeur, l'instant où il a été
/// obtenu et l'empreinte du jeton qui l'a ouvert.
struct Session {
    cookie: String,
    since: Instant,
    token: u64,
}

/// Garde le cookie d'administration de chaque cible.
#[derive(Default)]
pub struct VaultwardenCollector {
    sessions: Mutex<HashMap<TargetId, Session>>,
}

impl VaultwardenCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Le cookie gardé, s'il est récent *et* ouvert avec le jeton actuel : un
    /// jeton modifié doit être éprouvé tout de suite, pas quinze minutes plus tard.
    fn cached(&self, id: TargetId, admin_token: &str) -> Option<String> {
        let sessions = self.sessions.lock().ok()?;
        let session = sessions.get(&id)?;
        (session.since.elapsed() < SESSION_REUSE && session.token == fingerprint(admin_token))
            .then(|| session.cookie.clone())
    }

    fn remember(&self, id: TargetId, cookie: &str, admin_token: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(
                id,
                Session {
                    cookie: cookie.to_string(),
                    since: Instant::now(),
                    token: fingerprint(admin_token),
                },
            );
        }
    }

    /// Oublie le cookie d'une cible (refusé, ou jeton rejeté).
    fn forget(&self, id: TargetId) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(&id);
        }
    }

    async fn users(&self, target: &Target, admin_token: &str) -> Result<Value, ProbeError> {
        let options = Options::from_target(target, "https", DEFAULT_PORT)?;
        let http = admin_http(&options)?;
        if let Some(cookie) = self.cached(target.id, admin_token) {
            if let Some(users) = fetch_users(&http, &options, &cookie).await? {
                return Ok(users);
            }
            // 401, 403 ou redirection vers la page de connexion : la session est morte.
            self.forget(target.id);
        }
        let cookie = match admin_login(&http, &options, admin_token).await {
            Ok(cookie) => cookie,
            Err(error) => {
                self.forget(target.id);
                return Err(error);
            }
        };
        self.remember(target.id, &cookie, admin_token);
        match fetch_users(&http, &options, &cookie).await? {
            Some(users) => Ok(users),
            None => {
                self.forget(target.id);
                Err(ProbeError::Auth(
                    "Vaultwarden refused the admin session it just opened.".to_string(),
                ))
            }
        }
    }

    fn client(target: &Target) -> Result<HttpClient, ProbeError> {
        http_client(target, "https", DEFAULT_PORT, Auth::default(), PRODUCT)
    }
}

#[async_trait]
impl Collector for VaultwardenCollector {
    fn kind(&self) -> &str {
        "vaultwarden"
    }

    async fn probe(&self, target: &Target) -> Result<Vec<Sample>, ProbeError> {
        let ts_ms = now_ms();
        let client = Self::client(target)?;
        // La preuve de vie : le seul appel qui condamne.
        let (status, body) = client.get("/alive").await?;
        if !status.is_success() {
            return Err(client.status_error(status, &body, "/alive"));
        }
        let mut errors = 0u32;
        let mut out = vec![flag(&metric(P, "up"), true, ts_ms)];
        let config = client.get_json::<Value>("/api/config").await;
        let version =
            settle(fetch_version(&client).await, &mut errors, target.id, "api/version").flatten();
        if let Some(config) = settle(config, &mut errors, target.id, "api/config") {
            out.extend(config_samples(&config, version.as_deref(), ts_ms));
        }
        if let Some(admin_token) = token(&target.credential) {
            out.extend(user_samples(&self.users(target, admin_token).await?, ts_ms));
        }
        out.push(gauge(&metric(P, "scrape_errors"), f64::from(errors), ts_ms));
        Ok(out)
    }

    async fn discover(&self, target: &Target) -> Result<Option<String>, ProbeError> {
        let client = Self::client(target)?;
        let (status, body) = client.get("/alive").await?;
        if !status.is_success() {
            return Err(client.status_error(status, &body, "/alive"));
        }
        Ok(Some("vaultwarden".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use axum::Router;
    use axum::extract::Request;
    use axum::http::{HeaderMap, header};
    use axum::response::{IntoResponse, Redirect};
    use axum::routing::{get, post};
    use dumbmonit_proto::Credential;

    use super::*;

    // Réponses d'un Vaultwarden 1.34.3 : `/alive`, `/api/config` et la liste
    // des utilisateurs de l'administration (trois comptes : un sans second
    // facteur et un désactivé). Les adresses et identifiants sont fictifs.
    const ALIVE: &str = include_str!("testdata/vaultwarden_1.34.3/alive.json");
    const CONFIG: &str = include_str!("testdata/vaultwarden_1.34.3/config.json");
    const USERS: &str = include_str!("testdata/vaultwarden_1.34.3/admin_users.json");
    // Vaultwarden 1.37.5 : `/admin/users` y est en camelCase (`userEnabled`,
    // `twoFactorEnabled`) et ne donne plus `cipher_count` (clés relevées sur une
    // instance réelle, adresses et identifiants fictifs : un compte avec second
    // facteur, un sans, un désactivé) ; `/api/version` rend `"1.37.5"` alors que
    // `/api/config` annonce la version de compatibilité Bitwarden `2026.6.0`.
    const USERS_1_37: &str = include_str!("testdata/vaultwarden_1.37.5/admin_users.json");
    const VERSION: &str = include_str!("testdata/vaultwarden_1.37.5/version.json");

    fn value(samples: &[Sample], name: &str) -> Option<f64> {
        samples.iter().find(|s| s.metric == name).map(|s| s.value)
    }

    #[test]
    fn la_configuration_donne_la_version_et_l_etat_des_inscriptions() {
        // Sans `/api/version`, repli sur la version annoncée par la configuration.
        let samples = config_samples(&serde_json::from_str(CONFIG).unwrap(), None, 0);
        let info = samples.iter().find(|s| s.metric == "vaultwarden_version_info").unwrap();
        assert_eq!(info.labels["version"], "2025.5.0");
        assert_eq!(value(&samples, "vaultwarden_signups_open"), Some(0.0));
        let open =
            serde_json::json!({"version": "1", "settings": {"disableUserRegistration": false}});
        assert_eq!(value(&config_samples(&open, None, 0), "vaultwarden_signups_open"), Some(1.0));
    }

    #[test]
    fn la_version_est_celle_de_vaultwarden_et_pas_celle_de_compatibilite_bitwarden() {
        // Vaultwarden 1.37.5 : `/api/config` annonce 2026.6.0 (Bitwarden).
        let config = serde_json::json!({"version": "2026.6.0"});
        let version = server_version(VERSION);
        assert_eq!(version.as_deref(), Some("1.37.5"));
        let samples = config_samples(&config, version.as_deref(), 0);
        let info = samples.iter().find(|s| s.metric == "vaultwarden_version_info").unwrap();
        assert_eq!(info.labels["version"], "1.37.5");
        assert_eq!(server_version("1.34.3\n").as_deref(), Some("1.34.3"));
        assert_eq!(server_version("  "), None);
    }

    #[test]
    fn la_casse_camel_de_1_37_est_comprise_et_les_elements_ne_sont_pas_inventes() {
        let samples = user_samples(&serde_json::from_str(USERS_1_37).unwrap(), 0);
        assert_eq!(value(&samples, "vaultwarden_users"), Some(3.0));
        assert_eq!(value(&samples, "vaultwarden_users_disabled"), Some(1.0));
        // Bob seul : Eve est désactivée, Alice a un second facteur.
        assert_eq!(value(&samples, "vaultwarden_users_without_2fa"), Some(1.0));
        // `cipher_count` a disparu : pas de zéro trompeur.
        assert_eq!(value(&samples, "vaultwarden_items"), None);
        assert_eq!(value(&samples, "vaultwarden_admin_readable"), Some(1.0));
    }

    #[test]
    fn les_utilisateurs_ne_sortent_que_en_compteurs() {
        let samples = user_samples(&serde_json::from_str(USERS).unwrap(), 0);
        assert_eq!(value(&samples, "vaultwarden_users"), Some(3.0));
        assert_eq!(value(&samples, "vaultwarden_users_disabled"), Some(1.0));
        // Bob seul : Eve est désactivée, Alice a un second facteur.
        assert_eq!(value(&samples, "vaultwarden_users_without_2fa"), Some(1.0));
        assert_eq!(value(&samples, "vaultwarden_items"), Some(272.0));
        for sample in &samples {
            assert!(sample.labels.is_empty(), "{} porte une étiquette", sample.metric);
        }
    }

    #[test]
    fn une_liste_vide_donne_des_zeros() {
        let samples = user_samples(&serde_json::json!([]), 0);
        assert_eq!(value(&samples, "vaultwarden_users"), Some(0.0));
        assert_eq!(value(&samples, "vaultwarden_users_disabled"), Some(0.0));
        assert_eq!(value(&samples, "vaultwarden_items"), None);
    }

    async fn serve(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{address}")
    }

    fn target(address: String, credential: Credential) -> Target {
        Target {
            id: 21,
            name: "vault".into(),
            address,
            kind: "vaultwarden".into(),
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

    fn app(logins: Arc<AtomicUsize>) -> Router {
        app_with(logins, Arc::new(AtomicBool::new(false)), USERS)
    }

    /// `revoked` : la session admin déjà ouverte n'est plus acceptée (403).
    fn app_with(logins: Arc<AtomicUsize>, revoked: Arc<AtomicBool>, users: &'static str) -> Router {
        Router::new()
            .route("/alive", get(|| async { ALIVE }))
            .route("/api/config", get(|| async { CONFIG }))
            .route("/api/version", get(|| async { VERSION }))
            .route(
                "/admin",
                post(move |body: String| {
                    let logins = logins.clone();
                    async move {
                        if !body.contains("token=bon") {
                            return (
                                axum::http::StatusCode::UNAUTHORIZED,
                                "Invalid admin token, please try again.",
                            )
                                .into_response();
                        }
                        logins.fetch_add(1, Ordering::SeqCst);
                        let mut response = Redirect::to("/admin").into_response();
                        response.headers_mut().insert(
                            header::SET_COOKIE,
                            "VW_ADMIN=jwt; HttpOnly; SameSite=Strict; Path=/admin".parse().unwrap(),
                        );
                        response
                    }
                }),
            )
            .route(
                "/admin/users",
                get(move |request: Request| {
                    let revoked = revoked.clone();
                    async move {
                        let headers: &HeaderMap = request.headers();
                        let cookie = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
                        if revoked.load(Ordering::SeqCst) {
                            axum::http::StatusCode::FORBIDDEN.into_response()
                        } else if cookie == Some("VW_ADMIN=jwt") {
                            users.into_response()
                        } else {
                            Redirect::to("/admin").into_response()
                        }
                    }
                }),
            )
    }

    fn admin(token: &str) -> Credential {
        Credential::ApiToken { token: token.into() }
    }

    #[tokio::test]
    async fn sans_jeton_seuls_le_public_est_lu() {
        let base = serve(app(Arc::new(AtomicUsize::new(0)))).await;
        let samples =
            VaultwardenCollector::new().probe(&target(base, Credential::None)).await.unwrap();
        assert_eq!(value(&samples, "vaultwarden_up"), Some(1.0));
        assert!(value(&samples, "vaultwarden_users").is_none());
        assert!(value(&samples, "vaultwarden_admin_readable").is_none());
        assert_eq!(value(&samples, "vaultwarden_scrape_errors"), Some(0.0));
    }

    #[tokio::test]
    async fn le_cookie_d_administration_est_reutilise() {
        let logins = Arc::new(AtomicUsize::new(0));
        let base = serve(app(logins.clone())).await;
        let collector = VaultwardenCollector::new();
        for _ in 0..3 {
            let samples = collector.probe(&target(base.clone(), admin("bon"))).await.unwrap();
            assert_eq!(value(&samples, "vaultwarden_users"), Some(3.0));
        }
        assert_eq!(logins.load(Ordering::SeqCst), 1, "une seule ouverture de session admin");
    }

    #[tokio::test]
    async fn un_vaultwarden_1_37_donne_sa_version_et_des_comptes_justes() {
        let logins = Arc::new(AtomicUsize::new(0));
        let app = app_with(logins, Arc::new(AtomicBool::new(false)), USERS_1_37);
        let base = serve(app).await;
        let samples = VaultwardenCollector::new().probe(&target(base, admin("bon"))).await.unwrap();
        let info = samples.iter().find(|s| s.metric == "vaultwarden_version_info").unwrap();
        assert_eq!(info.labels["version"], "1.37.5");
        assert_eq!(value(&samples, "vaultwarden_users_disabled"), Some(1.0));
        assert_eq!(value(&samples, "vaultwarden_users_without_2fa"), Some(1.0));
        assert_eq!(value(&samples, "vaultwarden_scrape_errors"), Some(0.0));
    }

    #[tokio::test]
    async fn un_jeton_modifie_est_eprouve_sans_attendre_la_fin_du_cookie() {
        let logins = Arc::new(AtomicUsize::new(0));
        let base = serve(app(logins.clone())).await;
        let collector = VaultwardenCollector::new();
        collector.probe(&target(base.clone(), admin("bon"))).await.unwrap();
        // Même cible, jeton changé : le cookie gardé ne doit pas masquer le refus.
        let error = collector.probe(&target(base.clone(), admin("mauvais"))).await.unwrap_err();
        assert!(matches!(error, ProbeError::Auth(_)), "{error}");
        assert!(collector.cached(21, "bon").is_none(), "le cookie périmé est oublié");
        // Le bon jeton rétabli rouvre une session.
        collector.probe(&target(base, admin("bon"))).await.unwrap();
        assert_eq!(logins.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn un_cookie_refuse_est_oublie() {
        let logins = Arc::new(AtomicUsize::new(0));
        let revoked = Arc::new(AtomicBool::new(false));
        let base = serve(app_with(logins.clone(), revoked.clone(), USERS)).await;
        let collector = VaultwardenCollector::new();
        collector.probe(&target(base.clone(), admin("bon"))).await.unwrap();
        assert!(collector.cached(21, "bon").is_some());
        // 403 sur la session gardée et sur celle qu'on rouvre aussitôt.
        revoked.store(true, Ordering::SeqCst);
        let error = collector.probe(&target(base.clone(), admin("bon"))).await.unwrap_err();
        assert!(matches!(error, ProbeError::Auth(_)), "{error}");
        assert!(collector.cached(21, "bon").is_none());
        assert_eq!(logins.load(Ordering::SeqCst), 2, "une réouverture a été tentée");
        revoked.store(false, Ordering::SeqCst);
        collector.probe(&target(base, admin("bon"))).await.unwrap();
    }

    #[tokio::test]
    async fn un_mauvais_jeton_est_une_erreur_d_authentification() {
        let base = serve(app(Arc::new(AtomicUsize::new(0)))).await;
        let error =
            VaultwardenCollector::new().probe(&target(base, admin("mauvais"))).await.unwrap_err();
        assert!(matches!(error, ProbeError::Auth(_)));
        assert!(!error.means_down());
    }

    #[tokio::test]
    async fn un_coffre_eteint_est_une_panne() {
        let error = VaultwardenCollector::new()
            .probe(&target("http://127.0.0.1:1".into(), Credential::None))
            .await
            .unwrap_err();
        assert!(error.means_down());
    }
}
