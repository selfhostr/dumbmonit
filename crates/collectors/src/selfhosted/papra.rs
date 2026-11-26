//! Papra (archivage de documents auto-hébergé).
//!
//! Une clé d'API (`Authorization: Bearer ppapi_…`) avec deux permissions de
//! lecture, `organizations:read` et `documents:read`, et quatre routes :
//!
//! * `/api/ping` — publique, elle prouve que le serveur répond ;
//! * `/api/health` — publique aussi : l'état de la base de données (200 si
//!   elle répond, 500 sinon, avec le détail dans le corps) ;
//! * `/api/organizations` — les organisations de l'utilisateur qui porte la
//!   clé ; c'est la preuve que la clé est valable ;
//! * `/api/organizations/:id/documents/statistics` — par organisation, le
//!   nombre de documents et leur taille, corbeille comprise ou non.
//!
//! La panne que ce module existe pour voir : **la base de données qui ne
//! répond plus** alors que la page d'accueil reste servie. Les noms, les
//! contenus et les fichiers des documents ne sont jamais lus.
//!
//! Le premier appel (`/api/ping`) et le deuxième (`/api/organizations`)
//! condamnent la sonde ; les statistiques d'une organisation sont des
//! compléments, comptés dans `papra_scrape_errors` quand ils manquent.

use async_trait::async_trait;
use dumbmonit_proto::{Collector, ProbeError, Sample, Target};
use reqwest::StatusCode;
use serde_json::Value;

use super::client::{Auth, HttpClient};
use super::{
    MAX_NAMED, flag, gauge, http_client, metric, now_ms, number, optional_json, settle, token,
};

pub const DEFAULT_PORT: u16 = 1221;
const PRODUCT: &str = "Papra";
const P: &str = "papra";

// ------------------------------------------------------------ traduction

/// La base de données répond-elle ? `/api/health` donne 200 et `status: ok`
/// quand tout va bien, 500 sinon.
pub fn health_samples(status: StatusCode, body: &Value, ts_ms: i64) -> Vec<Sample> {
    let database = body.get("isDatabaseHealthy").and_then(Value::as_bool);
    let healthy = database.unwrap_or(status.is_success());
    vec![flag(&metric(P, "database_healthy"), healthy, ts_ms)]
}

/// Les organisations lisibles par la clé : `(identifiant, nom)`, au plus
/// [`MAX_NAMED`], et leur nombre total.
pub fn organizations(body: &Value) -> (usize, Vec<(String, String)>) {
    let list = body.get("organizations").and_then(Value::as_array);
    let all: Vec<&Value> = list.map(|l| l.iter().collect()).unwrap_or_default();
    let named = all
        .iter()
        .filter_map(|org| {
            let id = org.get("id")?.as_str()?.to_string();
            let name = org.get("name").and_then(Value::as_str).unwrap_or(&id).to_string();
            Some((id, name))
        })
        .take(MAX_NAMED)
        .collect();
    (all.len(), named)
}

/// Documents et taille d'une organisation. Les tailles sont en octets.
pub fn statistics_samples(organization: &str, body: &Value, ts_ms: i64) -> Vec<Sample> {
    let Some(stats) = body.get("organizationStats") else {
        return Vec::new();
    };
    [
        ("documents", "documentsCount"),
        ("documents_size_bytes", "documentsSize"),
        ("documents_deleted", "deletedDocumentsCount"),
        ("documents_deleted_size_bytes", "deletedDocumentsSize"),
    ]
    .into_iter()
    .filter_map(|(name, field)| {
        let value = number(stats.get(field))?;
        Some(gauge(&metric(P, name), value, ts_ms).with_label("organization", organization))
    })
    .collect()
}

// ------------------------------------------------------------------ sonde

pub async fn probe(
    client: &HttpClient,
    target: &Target,
    ts_ms: i64,
) -> Result<Vec<Sample>, ProbeError> {
    let _: Value = client.get_json("/api/ping").await?;
    let mut errors = 0u32;
    let mut out = Vec::new();

    // Une base indisponible répond 500 : c'est une mesure, pas une panne de transport.
    match client.get("/api/health").await {
        Ok((status, body)) => {
            let body = serde_json::from_str::<Value>(&body).unwrap_or(Value::Null);
            out.extend(health_samples(status, &body, ts_ms));
        }
        Err(error) => {
            errors += 1;
            tracing::warn!(target_id = target.id, %error, "santé de Papra indisponible");
        }
    }

    let orgs: Value = client.get_json("/api/organizations").await?;
    let (total, named) = organizations(&orgs);
    out.push(gauge(&metric(P, "organizations"), total as f64, ts_ms));
    let paths: Vec<String> = named
        .iter()
        .map(|(id, _)| format!("/api/organizations/{id}/documents/statistics"))
        .collect();
    let stats =
        futures::future::join_all(paths.iter().map(|path| optional_json(client, path))).await;
    for ((_, name), stats) in named.iter().zip(stats) {
        // Sans `documents:read` (403), la page est sautée : le reste se lit.
        if let Some(Some(body)) = settle(stats, &mut errors, target.id, "documents/statistics") {
            out.extend(statistics_samples(name, &body, ts_ms));
        }
    }
    out.push(gauge(&metric(P, "scrape_errors"), f64::from(errors), ts_ms));
    Ok(out)
}

// ------------------------------------------------------------ collecteur

#[derive(Default)]
pub struct PapraCollector;

impl PapraCollector {
    pub fn new() -> Self {
        Self
    }

    fn client(target: &Target) -> Result<HttpClient, ProbeError> {
        let Some(key) = token(&target.credential) else {
            return Err(ProbeError::Config(format!(
                "Papra expects an API key, configured: {}",
                target.credential
            )));
        };
        http_client(
            target,
            "http",
            DEFAULT_PORT,
            Auth::header("Authorization", format!("Bearer {key}")),
            PRODUCT,
        )
    }
}

#[async_trait]
impl Collector for PapraCollector {
    fn kind(&self) -> &str {
        "papra"
    }

    async fn probe(&self, target: &Target) -> Result<Vec<Sample>, ProbeError> {
        probe(&Self::client(target)?, target, now_ms()).await
    }

    async fn discover(&self, target: &Target) -> Result<Option<String>, ProbeError> {
        let _: Value = Self::client(target)?.get_json("/api/organizations").await?;
        Ok(Some("Papra".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use axum::Router;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::get;
    use dumbmonit_proto::Credential;

    use super::*;

    const ORGANIZATIONS: &str = r#"{"organizations":[
        {"id":"org_a1","name":"Maison","createdAt":"2025-01-01T00:00:00.000Z"},
        {"id":"org_b2","name":"Atelier","createdAt":"2025-02-01T00:00:00.000Z"}]}"#;
    const STATS_A: &str = r#"{"organizationStats":{"documentsCount":120,"documentsSize":5242880,
        "deletedDocumentsCount":3,"deletedDocumentsSize":1024,"totalDocumentsCount":123,
        "totalDocumentsSize":5243904}}"#;
    const STATS_B: &str = r#"{"organizationStats":{"documentsCount":7,"documentsSize":2048,
        "deletedDocumentsCount":0,"deletedDocumentsSize":0,"totalDocumentsCount":7,
        "totalDocumentsSize":2048}}"#;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn value(samples: &[Sample], name: &str, organization: Option<&str>) -> Option<f64> {
        samples
            .iter()
            .find(|s| {
                s.metric == name
                    && organization
                        .is_none_or(|o| s.labels.get("organization").is_some_and(|l| l == o))
            })
            .map(|s| s.value)
    }

    #[test]
    fn la_base_indisponible_se_lit_dans_le_corps() {
        let body = json(r#"{"isDatabaseHealthy":false,"isEverythingOk":false,"status":"error"}"#);
        let samples = health_samples(StatusCode::INTERNAL_SERVER_ERROR, &body, 0);
        assert_eq!(value(&samples, "papra_database_healthy", None), Some(0.0));
        let samples = health_samples(StatusCode::OK, &Value::Null, 0);
        assert_eq!(value(&samples, "papra_database_healthy", None), Some(1.0));
    }

    #[test]
    fn les_statistiques_se_posent_par_organisation() {
        let (total, named) = organizations(&json(ORGANIZATIONS));
        assert_eq!(total, 2);
        assert_eq!(named[0], ("org_a1".to_string(), "Maison".to_string()));
        let samples = statistics_samples("Maison", &json(STATS_A), 0);
        assert_eq!(value(&samples, "papra_documents", Some("Maison")), Some(120.0));
        let size = value(&samples, "papra_documents_size_bytes", Some("Maison"));
        assert_eq!(size, Some(5_242_880.0));
        assert_eq!(value(&samples, "papra_documents_deleted", Some("Maison")), Some(3.0));
        assert!(statistics_samples("x", &json("{}"), 0).is_empty());
    }

    async fn serve(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{address}")
    }

    fn target(address: String, key: &str) -> Target {
        Target {
            id: 9,
            name: "archives".into(),
            address,
            kind: "papra".into(),
            profile_id: None,
            parent_id: None,
            interval: Duration::from_secs(60),
            enabled: true,
            tags: BTreeMap::new(),
            credential: Credential::ApiToken { token: key.into() },
            group_name: String::new(),
            position: 0,
        }
    }

    fn app() -> Router {
        Router::new()
            .route("/api/ping", get(|| async { r#"{"status":"ok"}"# }))
            .route(
                "/api/health",
                get(|| async {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        r#"{"isDatabaseHealthy":false,"isEverythingOk":false,"status":"error"}"#,
                    )
                }),
            )
            .route(
                "/api/organizations",
                get(|headers: HeaderMap| async move {
                    let ok = headers.get("authorization").and_then(|v| v.to_str().ok())
                        == Some("Bearer ppapi_cle");
                    if ok {
                        (StatusCode::OK, ORGANIZATIONS)
                    } else {
                        (StatusCode::UNAUTHORIZED, r#"{"error":{"code":"user.unauthorized"}}"#)
                    }
                }),
            )
            .route("/api/organizations/org_a1/documents/statistics", get(|| async { STATS_A }))
            .route("/api/organizations/org_b2/documents/statistics", get(|| async { STATS_B }))
    }

    #[tokio::test]
    async fn sonde_complete() {
        let base = serve(app()).await;
        let samples = PapraCollector::new().probe(&target(base, "ppapi_cle")).await.unwrap();
        assert_eq!(value(&samples, "papra_database_healthy", None), Some(0.0));
        assert_eq!(value(&samples, "papra_organizations", None), Some(2.0));
        assert_eq!(value(&samples, "papra_documents", Some("Maison")), Some(120.0));
        assert_eq!(value(&samples, "papra_documents", Some("Atelier")), Some(7.0));
        assert_eq!(value(&samples, "papra_scrape_errors", None), Some(0.0));
    }

    #[tokio::test]
    async fn une_cle_refusee_est_une_erreur_d_authentification_et_un_hote_eteint_une_panne() {
        let base = serve(app()).await;
        let error = PapraCollector::new().probe(&target(base, "faux")).await.unwrap_err();
        assert!(matches!(error, ProbeError::Auth(_)));
        assert!(!error.means_down());
        let error = PapraCollector::new()
            .probe(&target("http://127.0.0.1:1".into(), "ppapi_cle"))
            .await
            .unwrap_err();
        assert!(error.means_down());
    }

    #[tokio::test]
    async fn sans_documents_read_les_statistiques_sont_sautees() {
        let app = Router::new()
            .route("/api/ping", get(|| async { r#"{"status":"ok"}"# }))
            .route("/api/health", get(|| async { r#"{"isDatabaseHealthy":true,"status":"ok"}"# }))
            .route("/api/organizations", get(|| async { ORGANIZATIONS }))
            .fallback(|| async { (StatusCode::FORBIDDEN, "{}") });
        let base = serve(app).await;
        let samples = PapraCollector::new().probe(&target(base, "ppapi_cle")).await.unwrap();
        assert_eq!(value(&samples, "papra_database_healthy", None), Some(1.0));
        assert!(value(&samples, "papra_documents", None).is_none());
        assert_eq!(value(&samples, "papra_scrape_errors", None), Some(0.0));
    }
}
