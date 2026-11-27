//! Comptes et sessions : liste « mes sessions » et fermeture, déconnexion d'un
//! compte partout, politique « second facteur obligatoire », codes de secours
//! régénérés, adresse électronique des comptes, et changements d'un jeton
//! d'API après sa création.

use crate::common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{PASSWORD, TestApp, VIEWER_PASSWORD};
use dumbmonit_server::auth::totp;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const RECOVERY: &str = "/api/auth/totp/recovery-codes";
const FIREFOX: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:133.0) Gecko/20100101 Firefox/133.0";

/// Connexion avec un `User-Agent`, comme un vrai navigateur ; rend le cookie.
async fn login_with_agent(app: &TestApp, username: &str, password: &str, agent: &str) -> String {
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::USER_AGENT, agent)
        .body(Body::from(json!({ "username": username, "password": password }).to_string()))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.expect("réponse");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap().to_string())
        .expect("cookie");
    let _ = response.into_body().collect().await;
    cookie
}

fn sessions(reply: &Value) -> Vec<Value> {
    reply.as_array().expect("liste des sessions").clone()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// Décode le secret base32 remis à l'enrôlement.
fn decode_base32(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = Vec::new();
    let mut buffer: u64 = 0;
    let mut bits = 0;
    for c in text.bytes() {
        let value = ALPHABET.iter().position(|&a| a == c).expect("caractère base32") as u64;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    out
}

/// Enrôle et active le second facteur ; rend le secret, les codes de secours et
/// le code TOTP déjà servi.
async fn enable_totp(app: &TestApp, cookie: &str) -> (Vec<u8>, Vec<String>, String) {
    let enrol =
        app.post("/api/auth/totp/enroll", json!({ "password": PASSWORD }), Some(cookie)).await;
    assert_eq!(enrol.status, StatusCode::OK, "{}", enrol.body);
    let secret = decode_base32(enrol.body["secret"].as_str().expect("secret"));
    let code = totp::code_at(&secret, now_secs());
    let verified = app.post("/api/auth/totp/verify", json!({ "code": code }), Some(cookie)).await;
    assert_eq!(verified.status, StatusCode::OK, "{}", verified.body);
    let codes = verified.body["recovery_codes"]
        .as_array()
        .expect("codes")
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    (secret, codes, code)
}

/// Un code valide qui n'est pas celui déjà servi.
fn fresh_code(secret: &[u8], used: &str) -> String {
    let now = now_secs();
    [now + 30, now, now.saturating_sub(30)]
        .into_iter()
        .map(|at| totp::code_at(secret, at))
        .find(|code| code != used)
        .expect("un pas distinct")
}

fn user_id(list: &Value, username: &str) -> i64 {
    let user = list.as_array().unwrap().iter().find(|u| u["username"] == username).expect("compte");
    user["id"].as_i64().unwrap()
}

async fn audit_actions(app: &TestApp, admin: &str) -> Vec<String> {
    let reply = app.get("/api/auth/audit", Some(admin)).await;
    reply.body.as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap().into()).collect()
}

#[tokio::test]
async fn each_account_lists_and_closes_its_own_sessions() {
    let app = TestApp::configured().await;
    let laptop = login_with_agent(&app, "admin", PASSWORD, FIREFOX).await;
    let phone = app.admin_cookie().await;

    let list = app.get("/api/auth/sessions", Some(&laptop)).await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);
    let list = sessions(&list.body);
    assert_eq!(list.len(), 2);
    let current: Vec<&Value> = list.iter().filter(|s| s["current"] == json!(true)).collect();
    assert_eq!(current.len(), 1, "exactly one session is the current one");
    assert_eq!(current[0]["browser"], json!("Firefox"));
    assert_eq!(current[0]["os"], json!("Linux"));
    assert_eq!(current[0]["via"], json!("password"));
    assert!(current[0]["last_seen_at"].is_string());
    for session in &list {
        assert!(session.get("token_hash").is_none(), "no secret in the list: {session}");
    }
    let laptop_id = current[0]["id"].as_str().unwrap().to_string();
    let phone_id = list.iter().find(|s| s["current"] == json!(false)).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // La session courante se ferme par « Se déconnecter », pas ici.
    let refused = app.delete(&format!("/api/auth/sessions/{laptop_id}"), Some(&laptop)).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);

    // La session d'un autre compte n'existe pas pour lui.
    let admin = laptop.clone();
    let viewer = app.viewer_cookie(&admin).await;
    let foreign = app.delete(&format!("/api/auth/sessions/{phone_id}"), Some(&viewer)).await;
    assert_eq!(foreign.status, StatusCode::NOT_FOUND, "{}", foreign.body);
    assert_eq!(app.get("/api/auth/me", Some(&phone)).await.status, StatusCode::OK);

    let closed = app.delete(&format!("/api/auth/sessions/{phone_id}"), Some(&laptop)).await;
    assert_eq!(closed.status, StatusCode::NO_CONTENT, "{}", closed.body);
    assert_eq!(app.get("/api/auth/me", Some(&phone)).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(app.get("/api/auth/me", Some(&laptop)).await.status, StatusCode::OK);
    assert!(audit_actions(&app, &laptop).await.contains(&"session.revoked".to_string()));
}

#[tokio::test]
async fn signing_out_the_other_sessions_keeps_this_one() {
    let app = TestApp::configured().await;
    let here = app.admin_cookie().await;
    let elsewhere = app.admin_cookie().await;
    let again = app.admin_cookie().await;

    let reply = app.post("/api/auth/sessions/revoke-others", json!({}), Some(&here)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["closed"], json!(2));
    assert_eq!(app.get("/api/auth/me", Some(&here)).await.status, StatusCode::OK);
    for gone in [&elsewhere, &again] {
        assert_eq!(app.get("/api/auth/me", Some(gone)).await.status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(sessions(&app.get("/api/auth/sessions", Some(&here)).await.body).len(), 1);

    // Un lecteur gère aussi ses propres sessions.
    let viewer = app.viewer_cookie(&here).await;
    let reply = app.post("/api/auth/sessions/revoke-others", json!({}), Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(app.get("/api/auth/sessions", Some(&viewer)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn an_admin_signs_a_user_out_everywhere() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;
    let viewer_again = app.login_as("viewer", VIEWER_PASSWORD).await.cookie();
    let users = app.get("/api/users", Some(&admin)).await;
    let viewer_id = user_id(&users.body, "viewer");
    let admin_id = user_id(&users.body, "admin");

    // Un lecteur ne déconnecte personne.
    let refused = app.delete(&format!("/api/users/{admin_id}/sessions"), Some(&viewer)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);

    let reply = app.delete(&format!("/api/users/{viewer_id}/sessions"), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["closed"], json!(2));
    for gone in [&viewer, &viewer_again] {
        assert_eq!(app.get("/api/auth/me", Some(gone)).await.status, StatusCode::UNAUTHORIZED);
    }

    // Pour son propre compte, la session qui demande survit.
    let other = app.admin_cookie().await;
    let reply = app.delete(&format!("/api/users/{admin_id}/sessions"), Some(&admin)).await;
    assert_eq!(reply.body["closed"], json!(1));
    assert_eq!(app.get("/api/auth/me", Some(&admin)).await.status, StatusCode::OK);
    assert_eq!(app.get("/api/auth/me", Some(&other)).await.status, StatusCode::UNAUTHORIZED);

    let unknown = app.delete("/api/users/999/sessions", Some(&admin)).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    assert!(audit_actions(&app, &admin).await.contains(&"user.signed_out".to_string()));
}

#[tokio::test]
async fn the_two_factor_policy_holds_staff_at_enrolment() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let viewer = app.viewer_cookie(&admin).await;

    assert_eq!(app.get("/api/auth/policy", Some(&admin)).await.body["require_totp"], json!(false));
    assert_eq!(app.get("/api/auth/policy", Some(&viewer)).await.status, StatusCode::FORBIDDEN);
    let refused = app.put("/api/auth/policy", json!({ "require_totp": true }), Some(&viewer)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);

    let saved = app.put("/api/auth/policy", json!({ "require_totp": true }), Some(&admin)).await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(saved.body["require_totp"], json!(true));

    // L'administrateur sans second facteur ne peut plus que l'enrôler.
    let blocked = app.get("/api/targets", Some(&admin)).await;
    assert_eq!(blocked.status, StatusCode::FORBIDDEN, "{}", blocked.body);
    assert_eq!(blocked.body["code"], json!("totp_setup_required"));
    let blocked = app.get("/api/auth/sessions", Some(&admin)).await;
    assert_eq!(blocked.body["code"], json!("totp_setup_required"));
    assert_eq!(app.get("/api/auth/me", Some(&admin)).await.status, StatusCode::OK);
    assert_eq!(app.get("/api/auth/totp", Some(&admin)).await.status, StatusCode::OK);
    let status = app.get("/api/auth/status", Some(&admin)).await;
    assert_eq!(status.body["totp_setup_required"], json!(true));

    // Un lecteur n'est pas concerné.
    assert_eq!(app.get("/api/targets", Some(&viewer)).await.status, StatusCode::OK);
    let status = app.get("/api/auth/status", Some(&viewer)).await;
    assert_eq!(status.body["totp_setup_required"], json!(false));

    enable_totp(&app, &admin).await;
    assert_eq!(app.get("/api/targets", Some(&admin)).await.status, StatusCode::OK);
    let status = app.get("/api/auth/status", Some(&admin)).await;
    assert_eq!(status.body["totp_setup_required"], json!(false));
    assert!(audit_actions(&app, &admin).await.contains(&"policy.updated".to_string()));

    // Coupée, la politique ne retient plus personne.
    let saved = app.put("/api/auth/policy", json!({}), Some(&admin)).await;
    assert_eq!(saved.body["require_totp"], json!(false));
}

#[tokio::test]
async fn recovery_codes_are_replaced_without_turning_two_factor_off() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;

    let off = app.post(RECOVERY, json!({ "password": PASSWORD }), Some(&admin)).await;
    assert_eq!(off.status, StatusCode::CONFLICT, "nothing to replace without 2FA");

    let (secret, old_codes, used) = enable_totp(&app, &admin).await;

    let empty = app.post(RECOVERY, json!({}), Some(&admin)).await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST, "{}", empty.body);
    let wrong = app.post(RECOVERY, json!({ "password": "faux" }), Some(&admin)).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    // Le code qui a activé le second facteur a déjà servi.
    let replayed = app.post(RECOVERY, json!({ "code": used }), Some(&admin)).await;
    assert_eq!(replayed.status, StatusCode::UNAUTHORIZED);

    let code = fresh_code(&secret, &used);
    let reply = app.post(RECOVERY, json!({ "code": code }), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let new_codes: Vec<String> = reply.body["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    assert_eq!(new_codes.len(), 8);
    assert!(new_codes.iter().all(|c| !old_codes.contains(c)));

    let status = app.get("/api/auth/totp", Some(&admin)).await;
    assert_eq!(status.body["enabled"], json!(true));
    assert_eq!(status.body["recovery_codes_left"], json!(8));

    // Un ancien code ne passe plus, un nouveau oui.
    let first_step = app.login_as("admin", PASSWORD).await;
    let pending = first_step.body["pending"].as_str().unwrap().to_string();
    let old = json!({ "pending": pending, "code": old_codes[0] });
    assert_eq!(app.post("/api/auth/login/totp", old, None).await.status, StatusCode::UNAUTHORIZED);
    let new = json!({ "pending": pending, "code": new_codes[0] });
    assert_eq!(app.post("/api/auth/login/totp", new, None).await.status, StatusCode::NO_CONTENT);

    // Le mot de passe suffit aussi.
    let reply = app.post(RECOVERY, json!({ "password": PASSWORD }), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert!(audit_actions(&app, &admin).await.contains(&"totp.recovery_regenerated".to_string()));
}

#[tokio::test]
async fn accounts_carry_an_optional_email_address() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;

    let bad = json!({ "username": "jane", "role": "viewer", "password": VIEWER_PASSWORD,
                      "email": "pas-une-adresse" });
    assert_eq!(app.post("/api/users", bad, Some(&admin)).await.status, StatusCode::BAD_REQUEST);

    let created = app
        .post(
            "/api/users",
            json!({ "username": "jane", "role": "viewer", "password": VIEWER_PASSWORD,
                    "email": " Jane@Example.ORG " }),
            Some(&admin),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["email"], json!("Jane@example.org"));
    let id = created.body["id"].as_i64().unwrap();

    // Absente : inchangée ; vide : effacée.
    let uri = format!("/api/users/{id}");
    let kept = app.put(&uri, json!({ "display_name": "Jane" }), Some(&admin)).await;
    assert_eq!(kept.body["email"], json!("Jane@example.org"));
    let bad = app.put(&uri, json!({ "email": "a@b" }), Some(&admin)).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let cleared = app.put(&uri, json!({ "email": "" }), Some(&admin)).await;
    assert_eq!(cleared.body["email"], Value::Null);

    // Jamais un identifiant de connexion.
    app.put(&uri, json!({ "email": "jane@example.org" }), Some(&admin)).await;
    let reply = app.login_as("jane@example.org", VIEWER_PASSWORD).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_api_token_is_renamed_restricted_and_extended_after_creation() {
    let app = TestApp::configured().await;
    let admin = app.admin_cookie().await;
    let created = app
        .post(
            "/api/tokens",
            json!({ "name": "Grafana", "scope": "read", "expires_in_days": 3 }),
            Some(&admin),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.body["id"].as_i64().unwrap();
    let secret = created.body["secret"].as_str().unwrap().to_string();
    let first_expiry = created.body["expires_at"].as_str().unwrap().to_string();
    let uri = format!("/api/tokens/{id}");

    let renamed = app
        .request(
            "PATCH",
            &uri,
            Some(json!({ "name": " Grafana (NAS) ", "allowed_networks": ["10.0.0.7/24"] })),
            Some(&admin),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(renamed.body["name"], json!("Grafana (NAS)"));
    assert_eq!(renamed.body["allowed_networks"], json!(["10.0.0.0/24"]));
    assert_eq!(renamed.body["expires_at"], json!(first_expiry), "an absent expiry is kept");
    assert_eq!(renamed.body["scope"], json!("read"));
    assert!(renamed.body.get("secret").is_none());

    let extended =
        app.request("PATCH", &uri, Some(json!({ "expires_in_days": 365 })), Some(&admin)).await;
    assert_eq!(extended.status, StatusCode::OK, "{}", extended.body);
    assert!(extended.body["expires_at"].as_str().unwrap() > first_expiry.as_str());
    let never =
        app.request("PATCH", &uri, Some(json!({ "expires_in_days": null })), Some(&admin)).await;
    assert_eq!(never.body["expires_at"], Value::Null);

    for bad in [
        json!({ "expires_in_days": 0 }),
        json!({ "expires_in_days": -3 }),
        json!({ "expires_in_days": 3651 }),
        json!({ "name": "  " }),
        json!({ "name": "x".repeat(81) }),
        json!({ "allowed_networks": ["pas-un-réseau"] }),
    ] {
        let reply = app.request("PATCH", &uri, Some(bad.clone()), Some(&admin)).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{bad}");
    }

    // Les jetons ne se modifient pas entre eux, et un lecteur ne modifie rien.
    let request = Request::builder()
        .method("PATCH")
        .uri(&uri)
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "name": "pirate" }).to_string()))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let viewer = app.viewer_cookie(&admin).await;
    let reply = app.request("PATCH", &uri, Some(json!({ "name": "lecteur" })), Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);

    assert_eq!(app.delete(&uri, Some(&admin)).await.status, StatusCode::NO_CONTENT);
    let revoked = app.request("PATCH", &uri, Some(json!({ "name": "x" })), Some(&admin)).await;
    assert_eq!(revoked.status, StatusCode::NOT_FOUND);
    assert!(audit_actions(&app, &admin).await.contains(&"token.updated".to_string()));
}
