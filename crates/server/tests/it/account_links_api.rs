//! Liens de compte : mot de passe oublié, lien de réinitialisation créé par un
//! administrateur, invitations. Ce qui compte ici : un lien ne sert qu'une
//! fois, expire, ferme les sessions du compte ; la demande ne dit jamais si un
//! compte existe ; les essais sont limités ; une invitation crée le compte
//! avec son rôle et ses équipes.

use crate::common;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dumbmonit_server::alerting::notify_policy::GlobalPolicy;
use dumbmonit_server::db;
use dumbmonit_server::notify::mail_server::{self, MailServer, PasswordChange};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tower::ServiceExt;

use common::TestApp;

struct Lab {
    app: TestApp,
    pool: SqlitePool,
    admin: String,
}

impl Lab {
    /// Écriture d'administrateur.
    async fn post(&self, uri: &str, body: Value) -> common::Reply {
        self.app.post(uri, body, Some(&self.admin)).await
    }

    async fn public(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        public_post(&self.app, uri, body).await
    }
}

const CHECK: &str = "/api/auth/password-reset/check";
const COMPLETE: &str = "/api/auth/password-reset/complete";
const FORGOT: &str = "/api/auth/password-reset";
const INVITES: &str = "/api/users/invitations";
const INVITE_CHECK: &str = "/api/auth/invitations/check";
const INVITE_ACCEPT: &str = "/api/auth/invitations/accept";

fn unknown_token() -> Value {
    json!({ "token": format!("{}.{}", "a".repeat(32), "b".repeat(64)) })
}

async fn lab() -> Lab {
    let dir = tempfile::tempdir().unwrap();
    let config = common::base_config(dir.path());
    let pool = db::open(&config.database_path()).await.unwrap();
    let app = common::build(dir, config, pool.clone()).await;
    app.create_admin().await;
    let admin = app.admin_cookie().await;
    Lab { app, pool, admin }
}

fn password_of(username: &str) -> String {
    format!("{username}-mot-de-passe-solide")
}

async fn create_user(lab: &Lab, username: &str, role: &str, email: Option<&str>) -> i64 {
    let password = password_of(username);
    let mut body = json!({ "username": username, "role": role, "password": password });
    if let Some(email) = email {
        body["email"] = json!(email);
    }
    let reply = lab.app.post("/api/users", body, Some(&lab.admin)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["id"].as_i64().unwrap()
}

/// Une écriture publique, comme l'interface l'envoie : sans cookie, avec la
/// preuve d'origine.
async fn public_post(app: &TestApp, uri: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-requested-with", "DumbMonit")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.expect("réponse");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("corps").to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn token_of(url: &str) -> String {
    url.split("token=").nth(1).expect("lien avec jeton").to_string()
}

async fn audit_actions(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar("SELECT action FROM audit_log ORDER BY id").fetch_all(pool).await.unwrap()
}

/// Serveur de courriel (injoignable : rien ne part vraiment) et URL publique.
async fn configure_mail(pool: &SqlitePool) {
    let cipher = db::init_cipher(pool, "secret-de-test-suffisamment-long").await.unwrap();
    let server = MailServer {
        host: "127.0.0.1".into(),
        port: Some(1),
        from_address: "monit@example.org".into(),
        ..MailServer::default()
    };
    mail_server::save(pool, &cipher, &server, PasswordChange::Keep).await.unwrap();
    let public_url = "https://monit.example".to_string();
    let policy = GlobalPolicy { public_url, ..GlobalPolicy::default() };
    dumbmonit_server::notify::policy_store::save_global(pool, &policy).await.unwrap();
}

#[tokio::test]
async fn a_reset_link_works_once_and_signs_the_account_out_everywhere() {
    let lab = lab().await;
    let id = create_user(&lab, "jane", "viewer", Some("jane@example.org")).await;
    let old_session = lab.app.login_as("jane", &password_of("jane")).await.cookie();

    let reply = lab.post(&format!("/api/users/{id}/reset-link"), json!({ "send": false })).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["emailed"], false);
    let url = reply.body["url"].as_str().unwrap();
    assert!(url.starts_with("https://") && url.contains("/reset?token="), "{url}");
    let token = token_of(url);

    // Seule l'empreinte du secret est en base.
    let stored: (String, Vec<u8>) = sqlx::query_as("SELECT id, token_hash FROM password_resets")
        .fetch_one(&lab.pool)
        .await
        .unwrap();
    let (stored_id, hash) = stored;
    let (public, secret) = token.split_once('.').unwrap();
    assert_eq!(stored_id, public);
    assert_eq!(hash.len(), 32);
    assert_ne!(hash, secret.as_bytes());

    let (status, body) = lab.public(CHECK, json!({ "token": token })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["username"], "jane");

    let short = json!({ "token": token, "password": "court" });
    let (status, _) = lab.public(COMPLETE, short).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "same strength rule as everywhere");

    let new_password = "un-tout-nouveau-mot-de-passe";
    let done = json!({ "token": token, "password": new_password });
    let (status, body) = lab.public(COMPLETE, done.clone()).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let me = lab.app.get("/api/auth/me", Some(&old_session)).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED, "every session is closed");
    let old = lab.app.login_as("jane", &password_of("jane")).await;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);
    assert_eq!(lab.app.login_as("jane", new_password).await.status, StatusCode::NO_CONTENT);

    // Une seconde fois : la même réponse qu'un lien inconnu.
    let (status, again) = lab.public(COMPLETE, done).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, unknown) = lab.public(CHECK, unknown_token()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(again, unknown);

    let actions = audit_actions(&lab.pool).await;
    for wanted in ["password_reset.link_created", "password_reset.completed"] {
        assert!(actions.iter().any(|a| a == wanted), "{wanted} in {actions:?}");
    }
}

#[tokio::test]
async fn a_new_link_replaces_the_previous_one_and_links_expire() {
    let lab = lab().await;
    let id = create_user(&lab, "jane", "operator", None).await;
    let uri = format!("/api/users/{id}/reset-link");
    let first = lab.post(&uri, json!({})).await;
    let second = lab.post(&uri, json!({})).await;
    let first = token_of(first.body["url"].as_str().unwrap());
    let second = token_of(second.body["url"].as_str().unwrap());

    let check = |token: String| json!({ "token": token });
    let (status, _) = lab.public(CHECK, check(first)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the first link was replaced");
    let (status, _) = lab.public(CHECK, check(second.clone())).await;
    assert_eq!(status, StatusCode::OK);

    sqlx::query("UPDATE password_resets SET expires_at = '2000-01-01 00:00:00'")
        .execute(&lab.pool)
        .await
        .unwrap();
    let (status, _) = lab.public(CHECK, check(second)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "expired");
}

#[tokio::test]
async fn wall_displays_and_disabled_accounts_get_no_reset_link() {
    let lab = lab().await;
    let wall = create_user(&lab, "mur", "wall", Some("wall@example.org")).await;
    let reply = lab.post(&format!("/api/users/{wall}/reset-link"), json!({})).await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.body);

    let jane = create_user(&lab, "jane", "viewer", None).await;
    let disable = json!({ "disabled": true });
    let reply = lab.app.put(&format!("/api/users/{jane}"), disable, Some(&lab.admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
    let reply = lab.post(&format!("/api/users/{jane}/reset-link"), json!({})).await;
    assert_eq!(reply.status, StatusCode::CONFLICT);

    // Un écran mural ne crée pas de lien, pas même pour lui.
    let screen = lab.app.login_as("mur", &password_of("mur")).await.cookie();
    let uri = format!("/api/users/{wall}/reset-link");
    let reply = lab.app.post(&uri, json!({}), Some(&screen)).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn without_a_mail_server_forgot_password_sends_nothing_and_says_the_same() {
    let lab = lab().await;
    create_user(&lab, "jane", "viewer", Some("jane@example.org")).await;
    let status = lab.app.get("/api/auth/status", None).await;
    assert_eq!(status.body["password_reset"], false);

    let (known, known_body) = lab.public(FORGOT, json!({ "login": "jane" })).await;
    let (unknown, unknown_body) = lab.public(FORGOT, json!({ "login": "nobody" })).await;
    assert_eq!(known, StatusCode::ACCEPTED);
    assert_eq!(unknown, StatusCode::ACCEPTED);
    assert_eq!(known_body, unknown_body);
    let issued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM password_resets")
        .fetch_one(&lab.pool)
        .await
        .unwrap();
    assert_eq!(issued, 0);
}

#[tokio::test]
async fn forgot_password_never_tells_whether_an_account_exists() {
    let lab = lab().await;
    configure_mail(&lab.pool).await;
    let jane = create_user(&lab, "jane", "viewer", Some("jane@example.org")).await;
    let paul = create_user(&lab, "paul", "viewer", None).await;
    let wall = create_user(&lab, "mur", "wall", Some("wall@example.org")).await;
    assert_eq!(lab.app.get("/api/auth/status", None).await.body["password_reset"], true);

    let mut answers = Vec::new();
    for login in ["jane", "paul", "nobody", "mur", "JANE@example.org"] {
        let started = Instant::now();
        let (status, body) = lab.public(FORGOT, json!({ "login": login })).await;
        let took = started.elapsed();
        assert_eq!(status, StatusCode::ACCEPTED, "{login}: {body}");
        assert!(took >= Duration::from_millis(400), "{login}: answered in {took:?}");
        answers.push(body);
    }
    assert!(answers.windows(2).all(|pair| pair[0] == pair[1]), "{answers:?}");

    let links = |user: i64| {
        let pool = lab.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM password_resets WHERE user_id = ? AND used_at IS NULL",
            )
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    assert_eq!(links(jane).await, 1, "by username, then by address: the last link only");
    assert_eq!(links(paul).await, 0, "no address");
    assert_eq!(links(wall).await, 0, "wall display");

    let actions = audit_actions(&lab.pool).await;
    assert!(actions.iter().any(|a| a == "password_reset.requested"));
    assert!(actions.iter().any(|a| a == "password_reset.ignored"));

    // Libre-service coupé : l'écran de connexion renvoie à la documentation.
    let off = json!({ "self_service": false });
    let reply = lab.app.put("/api/auth/recovery", off, Some(&lab.admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["self_service_available"], false);
    assert_eq!(lab.app.get("/api/auth/status", None).await.body["password_reset"], false);
}

#[tokio::test]
async fn reset_requests_and_link_guesses_are_rate_limited() {
    let lab = lab().await;
    let mut statuses = Vec::new();
    for _ in 0..7 {
        let (status, _) = lab.public(FORGOT, json!({ "login": "jane" })).await;
        statuses.push(status);
    }
    assert!(statuses[..6].iter().all(|s| *s == StatusCode::ACCEPTED), "{statuses:?}");
    assert_eq!(statuses[6], StatusCode::TOO_MANY_REQUESTS);

    let mut statuses = Vec::new();
    for _ in 0..7 {
        let (status, _) = lab.public(CHECK, unknown_token()).await;
        statuses.push(status);
    }
    assert!(statuses[..6].iter().all(|s| *s == StatusCode::NOT_FOUND), "{statuses:?}");
    assert_eq!(statuses[6], StatusCode::TOO_MANY_REQUESTS);
    // Le compteur des liens n'est pas celui de la connexion.
    assert_eq!(lab.app.login_as("admin", common::PASSWORD).await.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn public_link_routes_need_the_proof_of_origin() {
    let lab = lab().await;
    // `TestApp::post` sans cookie n'ajoute pas `X-Requested-With`.
    let reply = lab.app.post("/api/auth/password-reset", json!({ "login": "admin" }), None).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    let reply = lab.app.post(INVITE_ACCEPT, json!({ "token": "x" }), None).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_invitation_creates_the_account_with_its_role_and_teams() {
    let lab = lab().await;
    let team = lab.post("/api/teams", json!({ "name": "NOC", "grants": [] })).await;
    assert_eq!(team.status, StatusCode::CREATED, "{}", team.body);
    let team = team.body["id"].as_i64().unwrap();
    create_user(&lab, "jane", "viewer", Some("jane@example.org")).await;

    let refused = lab.post(INVITES, json!({ "email": "a@example.org", "role": "wall" })).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "no wall display by invitation");
    let taken = lab.post(INVITES, json!({ "email": "Jane@Example.org", "role": "viewer" })).await;
    assert_eq!(taken.status, StatusCode::CONFLICT, "an account already uses the address");

    let invite =
        json!({ "email": "Ada@Example.ORG", "role": "operator", "teams": [team], "send": false });
    let sent = lab.post(INVITES, invite).await;
    assert_eq!(sent.status, StatusCode::CREATED, "{}", sent.body);
    assert_eq!(sent.body["emailed"], false);
    assert_eq!(sent.body["invitation"]["email"], "Ada@example.org");
    assert_eq!(sent.body["invitation"]["teams"][0]["name"], "NOC");
    let url = sent.body["url"].as_str().unwrap();
    assert!(url.contains("/invite?token="), "{url}");
    let token = token_of(url);

    let list = lab.app.get(INVITES, Some(&lab.admin)).await;
    assert_eq!(list.body.as_array().map(Vec::len), Some(1));

    let (status, check) = lab.public(INVITE_CHECK, json!({ "token": token })).await;
    assert_eq!(status, StatusCode::OK, "{check}");
    assert_eq!(check["role"], "operator");
    assert_eq!(check["suggested_username"], "Ada");
    assert_eq!(check["sso"], false);

    let password = "le-mot-de-passe-d-ada";
    let clash = json!({ "token": token, "username": "admin", "password": password });
    let (status, _) = lab.public(INVITE_ACCEPT, clash).await;
    assert_eq!(status, StatusCode::CONFLICT, "username taken");
    let sso = json!({ "token": token, "sso": true });
    let (status, _) = lab.public(INVITE_ACCEPT, sso).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no single sign-on on this instance");

    let accept = json!({
        "token": token,
        "username": "ada",
        "display_name": "Ada L.",
        "password": password,
    });
    let (status, body) = lab.public(INVITE_ACCEPT, accept.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["username"], "ada");

    let session = lab.app.login_as("ada", password).await;
    assert_eq!(session.status, StatusCode::NO_CONTENT);
    let me = lab.app.get("/api/auth/me", Some(&session.cookie())).await;
    assert_eq!(me.body["role"], "operator");
    assert_eq!(me.body["email"], "Ada@example.org");
    assert_eq!(me.body["display_name"], "Ada L.");
    let teams = lab.app.get("/api/teams", Some(&lab.admin)).await;
    let members = teams.body["teams"][0]["members"]
        .as_array()
        .unwrap_or_else(|| panic!("{} {}", teams.status, teams.body));
    let ada = me.body["id"].as_i64();
    assert!(members.iter().any(|m| m["user_id"].as_i64() == ada), "{members:?}");

    let (status, _) = lab.public(INVITE_ACCEPT, accept).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "used once");
    let list = lab.app.get(INVITES, Some(&lab.admin)).await;
    assert_eq!(list.body.as_array().map(Vec::len), Some(0));

    let actions = audit_actions(&lab.pool).await;
    for wanted in ["invitation.created", "invitation.accepted"] {
        assert!(actions.iter().any(|a| a == wanted), "{wanted} in {actions:?}");
    }
}

#[tokio::test]
async fn an_invitation_can_be_resent_and_revoked() {
    let lab = lab().await;
    let sent = lab.post(INVITES, json!({ "email": "bob@example.org", "role": "viewer" })).await;
    assert_eq!(sent.status, StatusCode::CREATED);
    let id = sent.body["invitation"]["id"].as_i64().unwrap();
    let first = token_of(sent.body["url"].as_str().unwrap());
    // Sans serveur de courriel, l'envoi échoue proprement et le lien reste à copier.
    assert_eq!(sent.body["emailed"], false);
    assert!(sent.body["email_error"].is_string());

    let uri = format!("/api/users/invitations/{id}/resend");
    let resent = lab.post(&uri, json!({ "send": false })).await;
    assert_eq!(resent.status, StatusCode::OK, "{}", resent.body);
    let second = token_of(resent.body["url"].as_str().unwrap());

    let check = |token: String| json!({ "token": token });
    let (status, _) = lab.public(INVITE_CHECK, check(first)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the previous link stops working");
    let (status, _) = lab.public(INVITE_CHECK, check(second.clone())).await;
    assert_eq!(status, StatusCode::OK);

    let reply = lab.app.delete(&format!("/api/users/invitations/{id}"), Some(&lab.admin)).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let (status, _) = lab.public(INVITE_CHECK, check(second)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "revoked");
    let reply = lab.app.delete(&format!("/api/users/invitations/{id}"), Some(&lab.admin)).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);

    let viewer = lab.app.viewer_cookie(&lab.admin).await;
    let eve = json!({ "email": "eve@example.org", "role": "admin" });
    let reply = lab.app.post(INVITES, eve, Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "only an administrator invites");
}

#[tokio::test]
async fn the_link_settings_are_for_administrators_and_bounded() {
    let lab = lab().await;
    let viewer = lab.app.viewer_cookie(&lab.admin).await;
    let reply = lab.app.get("/api/auth/recovery", Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);

    let reply = lab.app.get("/api/auth/recovery", Some(&lab.admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["reset_minutes"], 30);
    assert_eq!(reply.body["invite_hours"], 168);
    assert_eq!(reply.body["mail_server"], false);
    // L'origine de l'administrateur sert de base aux liens.
    assert_eq!(reply.body["link_base"], "https://localhost:8080");

    let too_short = json!({ "reset_minutes": 2 });
    let bad = lab.app.put("/api/auth/recovery", too_short, Some(&lab.admin)).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let settings = json!({ "self_service": true, "reset_minutes": 60, "invite_hours": 48 });
    let reply = lab.app.put("/api/auth/recovery", settings, Some(&lab.admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["invite_hours"], 48);

    let sent = lab.post(INVITES, json!({ "email": "c@example.org", "role": "viewer" })).await;
    let read = |key: &str| {
        let raw = sent.body["invitation"][key].as_str().unwrap().to_string();
        chrono::NaiveDateTime::parse_from_str(&raw, "%Y-%m-%d %H:%M:%S").unwrap()
    };
    assert_eq!((read("expires_at") - read("created_at")).num_hours(), 48);
}
