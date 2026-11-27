//! « Mes notifications » : chacun règle les siennes, en session, quel que soit
//! son rôle ; ce qui est enregistré est vérifié et nettoyé ; le courriel exige
//! une adresse sur le compte.

use crate::common;

use axum::http::StatusCode;
use serde_json::json;

use common::TestApp;

const PREFS: &str = "/api/auth/notifications";

#[tokio::test]
async fn each_account_sets_its_own_notifications() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;

    let reply = app.get(PREFS, Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["prefs"]["email"], false);
    assert_eq!(reply.body["prefs"]["webpush"], false);
    assert_eq!(reply.body["prefs"]["min_severity"], "warning");
    assert_eq!(reply.body["email"], serde_json::Value::Null);
    assert_eq!(reply.body["mail_server"], false);
    assert_eq!(reply.body["push_devices"], 0);

    let refused = app.put(PREFS, json!({ "email": true }), Some(&viewer)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "no address on the account");

    let nothing_picked = json!({ "webpush": true, "scope": "pick" });
    let refused = app.put(PREFS, nothing_picked, Some(&viewer)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);

    let wanted = json!({
        "webpush": true,
        "min_severity": "critical",
        "scope": "pick",
        "folders": [" Paris// ", "Paris"],
        "tags": ["room = cave"],
    });
    let reply = app.put(PREFS, wanted, Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["prefs"]["folders"], json!(["Paris"]));
    assert_eq!(reply.body["prefs"]["tags"], json!(["room=cave"]));
    assert_eq!(reply.body["prefs"]["min_severity"], "critical");

    // Un essai sans appareil abonné le dit, sans erreur de requête.
    let test = app.post(&format!("{PREFS}/test"), json!({}), Some(&viewer)).await;
    assert_eq!(test.status, StatusCode::OK, "{}", test.body);
    assert_eq!(test.body["push"]["ok"], false);
    assert_eq!(test.body["email"], serde_json::Value::Null);

    // L'administrateur ajoute une adresse : le courriel devient possible.
    let users = app.get("/api/users", Some(&admin)).await;
    let list = users.body.as_array().unwrap();
    let row = list.iter().find(|u| u["username"] == "viewer").unwrap();
    let id = row["id"].as_i64().unwrap();
    let email = json!({ "email": "viewer@example.org" });
    let reply = app.put(&format!("/api/users/{id}"), email, Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
    let reply = app.put(PREFS, json!({ "email": true }), Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["email"], "viewer@example.org");

    // Les choix de l'un ne sont pas ceux de l'autre.
    let mine = app.get(PREFS, Some(&admin)).await;
    assert_eq!(mine.body["prefs"]["email"], false);

    // Sans session, rien.
    assert_eq!(app.get(PREFS, None).await.status, StatusCode::UNAUTHORIZED);
}
