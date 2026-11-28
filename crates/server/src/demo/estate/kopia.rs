//! Faux serveur Kopia (`kopia server`) : les réponses des tests du collecteur
//! (`kopia/testdata`), dates recalées. Un dépôt sur le NAS, quatre sources
//! dont une en cours d'envoi et une en pause, toutes sans erreur.

use axum::Router;
use axum::http::{StatusCode, Uri};
use axum::response::Response;

use super::{instant, json, replay};

const STATUS: &str = include_str!("../../../../collectors/src/kopia/testdata/repo_status.json");
const SOURCES: &str = include_str!("../../../../collectors/src/kopia/testdata/sources.json");

/// Instant des réponses enregistrées.
const CAPTURE: &str = "2026-10-26T01:00:00Z";

const WORDS: &[(&str, &str)] = &[
    ("\"hostname\":\"nas\"", "\"hostname\":\"nas.home.arpa\""),
    ("\"laptop\"", "\"laptop-ops2\""),
    // Parc calme : le dernier instantané des documents est sans erreur, celui
    // des archives date de la nuit, comme son planning quotidien le veut.
    ("\"ignoredErrorCount\":2,\"errorCount\":3", "\"ignoredErrorCount\":0,\"errorCount\":0"),
    ("\"numFailed\":3", "\"numFailed\":0"),
    (
        "\"startTime\":\"2026-10-20T01:00:00Z\",\"endTime\":\"2026-10-20T01:20:00Z\"",
        "\"startTime\":\"2026-10-25T04:00:00Z\",\"endTime\":\"2026-10-25T04:20:00Z\"",
    ),
    (
        "\"nextSnapshotTime\":\"2026-10-25T01:00:00Z\"",
        "\"nextSnapshotTime\":\"2026-10-26T04:00:00Z\"",
    ),
];

pub(super) fn router() -> Router {
    Router::new().fallback(handle)
}

async fn handle(uri: Uri) -> Response {
    let body = match uri.path().trim_end_matches('/') {
        "/api/v1/repo/status" => STATUS,
        "/api/v1/sources" => SOURCES,
        _ => return json(StatusCode::NOT_FOUND, "{}".into()),
    };
    replay(body, instant(CAPTURE), WORDS)
}
