//! Import de modèles par l'API : aperçu, application, droits et sauvegarde.

use crate::common;

use axum::http::StatusCode;
use serde_json::json;

/// Un `snmp.yml` minimal, écrit pour ce test (équipement fictif).
const SNMP_YML: &str = r#"
modules:
  acme_probe:
    walk: [1.3.6.1.4.1.99999.7]
    metrics:
      - name: acmeProbeTemperature
        oid: 1.3.6.1.4.1.99999.7.1
        type: gauge
      - name: acmeProbeSerial
        oid: 1.3.6.1.4.1.99999.7.2
        type: DisplayString
"#;

/// Un template qui ne contient que des clés d'agent Zabbix : rien à convertir.
const AGENT_ONLY: &str = r#"
zabbix_export:
  version: '7.0'
  templates:
    - template: Agent
      items:
        - name: CPU
          key: system.cpu.util
"#;

#[tokio::test]
async fn un_modele_se_previsualise_puis_sinstalle_comme_un_paquet() {
    let app = common::setup().await;
    app.create_admin().await;
    let cookie = app.admin_cookie().await;
    let body = json!({ "content": SNMP_YML, "filename": "snmp.yml" });

    let preview = app.post("/api/import/templates/preview", body.clone(), Some(&cookie)).await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.body);
    assert_eq!(preview.body["format"], "snmp_exporter_snmp");
    assert_eq!(preview.body["pack_id"], "acme-probe");
    assert_eq!(preview.body["installable"], true);
    assert_eq!(preview.body["exists"], false);
    assert_eq!(preview.body["summary"]["snmp_metrics"], 1);
    let report = preview.body["report"].as_array().expect("rapport");
    let serial = report.iter().find(|e| e["name"] == "acme_probe.acmeProbeSerial").unwrap();
    assert_eq!(serial["outcome"], "ignored");
    // L'aperçu n'installe rien.
    assert_eq!(app.get("/api/packs", Some(&cookie)).await.body.as_array().unwrap().len(), 0);

    let applied = app.post("/api/import/templates/apply", body.clone(), Some(&cookie)).await;
    assert_eq!(applied.status, StatusCode::CREATED, "{}", applied.body);
    assert_eq!(applied.body["pack_id"], "acme-probe");
    assert_eq!(applied.body["kind"], serde_json::Value::Null);
    assert_eq!(applied.body["snmp_profiles"], json!(["acme-probe"]));
    assert_eq!(applied.body["install"]["outcome"], "created");
    assert_eq!(applied.body["install"]["restart_required"], true);

    let packs = app.get("/api/packs", Some(&cookie)).await.body;
    assert_eq!(packs[0]["id"], "acme-probe");
    let again = app.post("/api/import/templates/preview", body.clone(), Some(&cookie)).await;
    assert_eq!(again.body["exists"], true);
    let same = app.post("/api/import/templates/apply", body, Some(&cookie)).await;
    assert_eq!(same.status, StatusCode::OK, "{}", same.body);
    assert_eq!(same.body["install"]["outcome"], "unchanged");
}

#[tokio::test]
async fn le_paquet_importe_figure_dans_la_sauvegarde() {
    const PHRASE: &str = "une phrase de passe assez longue";
    let app = common::setup().await;
    app.create_admin().await;
    let cookie = app.admin_cookie().await;
    let applied = app
        .post("/api/import/templates/apply", json!({ "content": SNMP_YML }), Some(&cookie))
        .await;
    assert_eq!(applied.status, StatusCode::CREATED, "{}", applied.body);

    let bundle = app.post("/api/backup", json!({ "passphrase": PHRASE }), Some(&cookie)).await;
    assert_eq!(bundle.status, StatusCode::OK, "{}", bundle.body);
    assert_eq!(bundle.body["summary"]["packs"], 1);

    let fresh = common::setup().await;
    fresh.create_admin().await;
    let fresh_cookie = fresh.admin_cookie().await;
    let restored = fresh
        .post(
            "/api/backup/restore",
            json!({ "bundle": bundle.body, "passphrase": PHRASE, "apply": true }),
            Some(&fresh_cookie),
        )
        .await;
    assert_eq!(restored.status, StatusCode::OK, "{}", restored.body);
    let packs = fresh.get("/api/packs", Some(&fresh_cookie)).await.body;
    assert_eq!(packs[0]["id"], "acme-probe");
    assert_eq!(packs[0]["snmp_profiles"], json!(["acme-probe"]));
}

#[tokio::test]
async fn seul_un_administrateur_importe_et_un_fichier_illisible_est_refuse() {
    let app = common::setup().await;
    app.create_admin().await;
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;
    let body = json!({ "content": SNMP_YML });

    for uri in ["/api/import/templates/preview", "/api/import/templates/apply"] {
        assert_eq!(app.post(uri, body.clone(), Some(&viewer)).await.status, StatusCode::FORBIDDEN);
        assert_eq!(app.post(uri, body.clone(), None).await.status, StatusCode::UNAUTHORIZED);
    }

    let unknown = json!({ "content": "hello: world\n" });
    let refused = app.post("/api/import/templates/preview", unknown, Some(&admin)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{}", refused.body);

    // Rien de convertible : l'aperçu le dit, l'application refuse.
    let agent_only = json!({ "content": AGENT_ONLY });
    let preview = app.post("/api/import/templates/preview", agent_only.clone(), Some(&admin)).await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.body);
    assert_eq!(preview.body["installable"], false);
    let apply = app.post("/api/import/templates/apply", agent_only, Some(&admin)).await;
    assert_eq!(apply.status, StatusCode::BAD_REQUEST, "{}", apply.body);
    assert_eq!(app.get("/api/packs", Some(&admin)).await.body.as_array().unwrap().len(), 0);

    let extra = json!({ "content": SNMP_YML, "unknown": 1 });
    let rejected = app.post("/api/import/templates/preview", extra, Some(&admin)).await;
    assert_eq!(rejected.status, StatusCode::BAD_REQUEST);
}
