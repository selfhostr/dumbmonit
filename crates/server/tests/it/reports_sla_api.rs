//! Rapport SLA : téléchargement à la demande (PDF, CSV) et calendriers de type
//! `sla`. VictoriaMetrics est injoignable ici : le rapport se construit quand
//! même, sans chiffres de disponibilité.

use crate::common;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::TestApp;

struct Raw {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
}

async fn get(app: &TestApp, uri: &str, cookie: Option<&str>) -> Raw {
    let mut builder = Request::builder().uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app.router.clone().oneshot(builder.body(Body::empty()).unwrap()).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.expect("corps").to_bytes().to_vec();
    Raw { status, headers, bytes }
}

#[tokio::test]
async fn a_pdf_is_built_for_a_chosen_period() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let uri = "/api/reports/sla?from=2024-10-01&to=2024-10-31&format=pdf&objective=99.9";
    let reply = get(&app, uri, Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.headers[header::CONTENT_TYPE], "application/pdf");
    assert_eq!(
        reply.headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"availability-2024-10.pdf\""
    );
    assert!(reply.bytes.starts_with(b"%PDF-1.4"));
    let text = String::from_utf8_lossy(&reply.bytes);
    assert!(text.contains("2024-10-01 to 2024-10-31 \\(UTC\\)"), "{text}");
}

#[tokio::test]
async fn the_default_period_is_the_previous_calendar_month() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let reply = get(&app, "/api/reports/sla?format=csv", Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.headers[header::CONTENT_TYPE], "text/csv; charset=utf-8");
    let disposition = reply.headers[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(disposition.contains("availability-") && disposition.ends_with("-monitors.csv\""));
    let text = String::from_utf8_lossy(&reply.bytes);
    assert!(text.starts_with("\u{feff}group,monitor,availability_percent"), "{text}");
}

#[tokio::test]
async fn the_csv_lists_monitors_and_the_incident_csv_has_its_header() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let created = app
        .post(
            "/api/targets",
            json!({ "name": "NAS, main", "address": "10.0.0.5", "kind": "dummy" }),
            Some(&admin),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let monitors =
        get(&app, "/api/reports/sla?from=2024-10-01&to=2024-10-31&format=csv", Some(&admin)).await;
    let text = String::from_utf8_lossy(&monitors.bytes);
    assert!(text.contains("\"NAS, main\""), "{text}");
    let incidents = get(
        &app,
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&format=incidents_csv",
        Some(&admin),
    )
    .await;
    let text = String::from_utf8_lossy(&incidents.bytes);
    assert!(text.contains("monitor,rule,severity,started_utc"), "{text}");
}

#[tokio::test]
async fn bad_requests_are_refused_with_a_reason() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    for uri in [
        "/api/reports/sla?from=2024-10-31&to=2024-10-01",
        "/api/reports/sla?from=2023-01-01&to=2024-12-31",
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&format=docx",
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&tz=Mars/Olympus",
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&objective=0",
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&objective=101",
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&page=9999",
        "/api/reports/sla?from=yesterday",
        "/api/reports/sla?to=2024-10-31",
        "/api/reports/sla?from=2099-01-01&to=2099-01-31",
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&template=fancy",
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&paper=a3",
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&sections=summary,bogus",
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&scope=not-json",
    ] {
        let reply = get(&app, uri, Some(&admin)).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{uri}");
    }
}

#[tokio::test]
async fn only_administrators_can_download() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;
    let uri = "/api/reports/sla?from=2024-10-01&to=2024-10-31";
    assert_eq!(get(&app, uri, None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(get(&app, uri, Some(&viewer)).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_sla_schedule_is_monthly_and_keeps_its_options() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let weekly = app
        .post(
            "/api/reports/schedules",
            json!({ "name": "SLA", "kind": "sla", "frequency": "weekly", "enabled": false }),
            Some(&admin),
        )
        .await;
    assert_eq!(weekly.status, StatusCode::BAD_REQUEST, "{}", weekly.body);

    let page = app
        .post("/api/status-pages", json!({ "title": "Acme", "slug": "acme" }), Some(&admin))
        .await;
    let page_id = page.body["id"].as_i64().unwrap();
    let created = app
        .post(
            "/api/reports/schedules",
            json!({ "name": "Monthly SLA", "kind": "sla", "frequency": "monthly", "day_of_month": 2,
                    "enabled": false, "attach_csv": false, "objective": 99.9, "page_id": page_id }),
            Some(&admin),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["kind"], "sla");
    assert_eq!(created.body["attach_pdf"], true);
    assert_eq!(created.body["attach_csv"], false);
    assert_eq!(created.body["objective"], 99.9);
    assert_eq!(created.body["page_id"], page_id);

    let digest = app
        .post("/api/reports/schedules", json!({ "name": "Weekly", "enabled": false }), Some(&admin))
        .await;
    assert_eq!(digest.body["kind"], "digest", "the default is the summary");

    for bad in [json!({ "kind": "other" }), json!({ "objective": 150 }), json!({ "page_id": 9999 })]
    {
        let mut body = json!({ "name": "x", "enabled": false, "frequency": "monthly" });
        body.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        let reply = app.post("/api/reports/schedules", body, Some(&admin)).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn the_summary_template_and_the_paper_follow_the_request() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let uri = "/api/reports/sla?from=2024-10-01&to=2024-10-31&template=summary&paper=letter\
               &sections=summary,incidents&title=Board%20pack&notes=Hidden";
    let reply = get(&app, uri, Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
    let text = String::from_utf8_lossy(&reply.bytes);
    assert!(text.contains("/MediaBox [0 0 612 792]"), "Letter paper");
    assert!(text.contains("(Major incidents)"));
    assert!(text.contains("(Board pack)"));
    assert!(!text.contains("(Availability by monitor)"), "monitors were not asked for");
    assert!(!text.contains("Hidden"), "notes were not asked for");
}

#[tokio::test]
async fn the_json_view_follows_the_scope_and_the_layout() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let mut ids = Vec::new();
    for (name, address) in [("Web", "10.0.0.7"), ("Mail", "10.0.0.8")] {
        let created = app
            .post(
                "/api/targets",
                json!({ "name": name, "address": address, "kind": "dummy" }),
                Some(&admin),
            )
            .await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
        ids.push(created.body["id"].as_i64().unwrap());
    }
    let all =
        app.get("/api/reports/sla?from=2024-10-01&to=2024-10-31&format=json", Some(&admin)).await;
    assert_eq!(all.status, StatusCode::OK, "{}", all.body);
    assert_eq!(all.body["monitors"].as_array().unwrap().len(), 2);
    assert_eq!(all.body["layout"]["template"], "detailed");
    assert_eq!(all.body["title"], "Availability report");

    // `{"targets":[id]}`, encodé pour l'adresse.
    let uri = format!(
        "/api/reports/sla?from=2024-10-01&to=2024-10-31&format=json&template=summary\
         &objective=99.5&scope=%7B%22targets%22%3A%5B{}%5D%7D",
        ids[0]
    );
    let scoped = app.get(&uri, Some(&admin)).await;
    assert_eq!(scoped.status, StatusCode::OK, "{}", scoped.body);
    let monitors = scoped.body["monitors"].as_array().unwrap();
    assert_eq!(monitors.len(), 1);
    assert_eq!(monitors[0]["name"], "Web");
    assert_eq!(scoped.body["layout"]["template"], "summary");
    assert_eq!(scoped.body["objective"], 99.5);
    assert_eq!(scoped.body["fleet_status"], "No data", "no time-series database here");
}

#[tokio::test]
async fn a_schedule_keeps_its_layout_and_scope() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let created = app
        .post(
            "/api/reports/schedules",
            json!({
                "name": "Board SLA", "kind": "sla", "frequency": "monthly", "enabled": false,
                "layout": { "template": "summary", "paper": "letter", "title": "Board pack",
                            "notes": "Quiet month.", "show_logo": false,
                            "sections": { "chart": false } },
                "scope": { "targets": [3, 3, 1], "groups": ["Edge", " Edge "] }
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let layout = &created.body["layout"];
    assert_eq!(layout["template"], "summary");
    assert_eq!(layout["paper"], "letter");
    assert_eq!(layout["title"], "Board pack");
    assert_eq!(layout["show_logo"], false);
    assert_eq!(layout["sections"]["chart"], false);
    assert_eq!(layout["sections"]["incidents"], true, "unnamed sections stay in");
    assert_eq!(created.body["scope"], json!({ "targets": [1, 3], "groups": ["Edge"] }));

    let id = created.body["id"].as_i64().unwrap();
    let read = app.get(&format!("/api/reports/schedules/{id}"), Some(&admin)).await;
    assert_eq!(read.body["layout"], created.body["layout"]);

    let too_long = app
        .post(
            "/api/reports/schedules",
            json!({ "name": "Bad", "kind": "sla", "frequency": "monthly", "enabled": false,
                    "layout": { "notes": "n".repeat(2001) } }),
            Some(&admin),
        )
        .await;
    assert_eq!(too_long.status, StatusCode::BAD_REQUEST);
}

/// Un JPEG minimal : SOI, APP0, SOF0 (8 bits, 3×2, 3 composantes), EOI.
fn tiny_jpeg() -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
    bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 8, 0, 2, 0, 3, 3]);
    bytes.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1]);
    bytes.extend_from_slice(&[0xFF, 0xD9]);
    bytes
}

#[tokio::test]
async fn the_report_logo_is_uploaded_and_chosen_by_the_layout() {
    use base64::Engine as _;
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let base = "/api/reports/sla?from=2024-10-01&to=2024-10-31&format=json";

    // Rien de téléversé : `custom` n'a pas de logo, et l'adresse répond 404.
    let none = app.get(&format!("{base}&logo=custom"), Some(&admin)).await;
    assert_eq!(none.status, StatusCode::OK, "{}", none.body);
    assert!(none.body["logo_url"].is_null());
    assert_eq!(none.body["layout"]["logo"], "custom");
    let missing = get(&app, "/api/reports/logo", Some(&admin)).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    // Le WebP ne s'incorpore pas au PDF : refusé dès le dépôt.
    let webp = base64::engine::general_purpose::STANDARD.encode(b"RIFF\0\0\0\0WEBPVP8 ");
    let refused = app.put("/api/reports/logo", json!({ "data": webp }), Some(&admin)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{}", refused.body);

    let data = format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(tiny_jpeg())
    );
    let viewer = app.viewer_cookie(&admin).await;
    let denied = app.put("/api/reports/logo", json!({ "data": data }), Some(&viewer)).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN, "{}", denied.body);
    let stored = app.put("/api/reports/logo", json!({ "data": data }), Some(&admin)).await;
    assert_eq!(stored.status, StatusCode::NO_CONTENT, "{}", stored.body);

    let image = get(&app, "/api/reports/logo", Some(&admin)).await;
    assert_eq!(image.status, StatusCode::OK);
    assert_eq!(image.headers[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(image.bytes, tiny_jpeg());

    let custom = format!("{base}&logo=custom");
    let chosen = app.get(&custom, Some(&admin)).await;
    assert_eq!(chosen.body["logo_url"], "/reports/logo");
    let pdf_uri = "/api/reports/sla?from=2024-10-01&to=2024-10-31&logo=custom";
    let pdf = get(&app, pdf_uri, Some(&admin)).await;
    assert!(String::from_utf8_lossy(&pdf.bytes).contains("/XObject"), "the PDF carries it");
    // `auto` sans client ni page : pas de logo ; `false` (ancienne adresse) non plus.
    let automatic = app.get(base, Some(&admin)).await;
    assert!(automatic.body["logo_url"].is_null());
    let hidden = app.get(&format!("{base}&logo=false"), Some(&admin)).await;
    assert_eq!(hidden.body["layout"]["logo"], "none");
    assert_eq!(hidden.body["layout"]["show_logo"], false);
    let unknown = app.get(&format!("{base}&logo=svg"), Some(&admin)).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);

    let removed = app.delete("/api/reports/logo", Some(&admin)).await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    let gone = app.get(&custom, Some(&admin)).await;
    assert!(gone.body["logo_url"].is_null());
}
