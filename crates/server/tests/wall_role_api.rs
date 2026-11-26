//! Le rôle `wall` : un compte d'écran mural, qui ne voit que le mode mur.
//!
//! Il lit ce que lit `/wall` (liste blanche `WALL_ROUTES`) et rien d'autre :
//! chaque opération de la spécification OpenAPI hors de cette liste lui est
//! refusée, lectures comprises. Sa session se renouvelle tant qu'il affiche le
//! mur et se ferme s'il change de rôle ; la politique du second facteur ne le
//! bloque pas ; une équipe limite ce qu'il montre ; ses jetons n'ouvrent rien ;
//! il voyage dans la sauvegarde.

mod common;

use axum::http::StatusCode;
use chrono::{Duration, SecondsFormat, Utc};
use common::TestApp;
use dumbmonit_server::auth::session;
use dumbmonit_server::auth::users::{self, NewUser, Role};
use dumbmonit_server::backup::{ExportOptions, collect, restore};
use dumbmonit_server::db;
use serde_json::{Value, json};
use sqlx::SqlitePool;

const WALL_PASSWORD: &str = "mot-de-passe-de-l-ecran-mural";

/// `Max-Age` du cookie : la durée de vie des sessions (`DUMBMONIT_SESSION_DAYS`),
/// la même pour un écran mural que pour tout compte.
fn max_age() -> String {
    format!("Max-Age={}", session::lifetime_days() * 86_400)
}

struct Lab {
    app: TestApp,
    pool: SqlitePool,
    admin: String,
    wall_id: i64,
    wall: String,
}

async fn lab() -> Lab {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let config = common::base_config(dir.path());
    let pool = db::open(&config.database_path()).await.expect("ouverture de la base");
    db::alerts::seed_builtin_rules(&pool).await.expect("règles intégrées");
    let app = common::build(dir, config, pool.clone()).await;
    app.create_admin().await;
    let admin = app.admin_cookie().await;

    for (name, address, folder) in [("nas", "10.0.0.7", "Paris"), ("router", "10.0.0.1", "Lyon")] {
        let reply = app
            .post(
                "/api/targets",
                json!({ "name": name, "address": address, "kind": "dummy", "group_name": folder }),
                Some(&admin),
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    }

    let reply = app
        .post(
            "/api/users",
            json!({ "username": "lobby", "role": "wall", "password": WALL_PASSWORD }),
            Some(&admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    assert_eq!(reply.body["role"], json!("wall"));
    let wall_id = reply.body["id"].as_i64().expect("id");

    let login = app.login_as("lobby", WALL_PASSWORD).await;
    assert_eq!(login.status, StatusCode::NO_CONTENT, "{}", login.body);
    let posed = login.set_cookie.clone().expect("cookie posé");
    assert!(posed.contains(&max_age()), "durée ordinaire à la connexion : {posed}");
    let wall = login.cookie();

    Lab { app, pool, admin, wall_id, wall }
}

#[tokio::test]
async fn a_wall_display_reads_what_the_wall_shows() {
    let lab = lab().await;
    let app = &lab.app;

    let me = app.get("/api/auth/me", Some(&lab.wall)).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.body);
    assert_eq!(me.body["role"], json!("wall"));

    // Les équipements, sans adresse ni options : le mur ne les montre pas.
    let targets = app.get("/api/targets", Some(&lab.wall)).await;
    assert_eq!(targets.status, StatusCode::OK, "{}", targets.body);
    let list = targets.body.as_array().expect("liste");
    assert_eq!(list.len(), 2);
    for target in list {
        assert_eq!(target["address"], json!(""), "{target}");
        assert_eq!(target["tags"], json!({}), "{target}");
        assert!(target["name"].is_string());
    }
    assert!(!targets.body.to_string().contains("10.0.0."), "aucune adresse : {}", targets.body);
    // Pas davantage le détail de l'état du serveur que `/api/health` rend à
    // une session ordinaire.
    let health = app.get("/api/health", Some(&lab.wall)).await;
    assert!(health.body.get("data_disk").is_none_or(Value::is_null), "{}", health.body);
    // Un administrateur, lui, les voit toujours.
    let full = app.get("/api/targets", Some(&lab.admin)).await;
    assert!(full.body.to_string().contains("10.0.0.7"));

    for uri in [
        "/api/alerts",
        "/api/alerts/rules",
        "/api/alerts/silences",
        "/api/incidents",
        "/api/instance/settings",
        "/api/wall/weather",
    ] {
        let reply = app.get(uri, Some(&lab.wall)).await;
        assert_eq!(reply.status, StatusCode::OK, "{uri}: {}", reply.body);
    }
    // VictoriaMetrics est injoignable dans les tests : la route répond, le
    // garde ne refuse pas.
    let metrics = app.get("/api/metrics/query?query=up", Some(&lab.wall)).await;
    assert!(
        !matches!(metrics.status, StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED),
        "{}: {}",
        metrics.status,
        metrics.body
    );

    // Se déconnecter reste possible.
    let out = app.post("/api/auth/logout", json!({}), Some(&lab.wall)).await;
    assert_eq!(out.status, StatusCode::NO_CONTENT, "{}", out.body);
    assert_eq!(app.get("/api/auth/me", Some(&lab.wall)).await.status, StatusCode::UNAUTHORIZED);
}

/// Toute opération documentée hors de la liste du mur est refusée au compte
/// mural, quelle que soit sa méthode : la spécification OpenAPI sert d'inventaire
/// (son test garantit qu'elle décrit chaque route du routeur).
#[tokio::test]
async fn every_other_route_is_refused_to_a_wall_display() {
    let lab = lab().await;
    let app = &lab.app;
    let spec = app.get("/api/openapi.json", None).await;
    assert_eq!(spec.status, StatusCode::OK);

    let mut problems = Vec::new();
    let mut checked = 0;
    for (path, item) in spec.body["paths"].as_object().expect("paths") {
        for method in ["get", "post", "put", "delete", "patch"] {
            let Some(op) = item.get(method) else { continue };
            // Routes publiques : sans session, par conception.
            if op.get("security").and_then(Value::as_array).is_some_and(Vec::is_empty) {
                continue;
            }
            if op["x-wall"].as_bool() == Some(true) {
                continue;
            }
            let uri: String = path
                .split('/')
                .map(|segment| if segment.starts_with('{') { "1" } else { segment })
                .collect::<Vec<_>>()
                .join("/");
            let verb = method.to_uppercase();
            let body = (method != "get").then(|| json!({}));
            let reply = app.request(&verb, &uri, body, Some(&lab.wall)).await;
            checked += 1;
            // Sous le garde de session : le refus du mur, et lui seul. Les
            // autres (SCIM, MCP, Prometheus, OTLP) veulent un jeton : le
            // cookie n'y ouvre rien.
            // Éteints par défaut (SCIM, OTLP), ils ne sont même pas montés : 404.
            let session_guarded = path.starts_with("/api/")
                && !path.starts_with("/api/scim")
                && !path.starts_with("/api/otlp")
                && path != "/api/mcp";
            let refused = if session_guarded {
                reply.status == StatusCode::FORBIDDEN
                    && reply.body["error"].as_str().is_some_and(|e| e.contains("wall display"))
            } else {
                matches!(
                    reply.status,
                    StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED | StatusCode::NOT_FOUND
                )
            };
            if !refused {
                problems.push(format!("{verb} {uri} : {} {}", reply.status, reply.body));
            }
        }
    }
    assert!(checked > 200, "inventaire suspect : {checked} opérations");
    assert!(problems.is_empty(), "ouvert à un écran mural :\n{}", problems.join("\n"));

    // Aucun de ces refus n'a touché à la session.
    assert_eq!(app.get("/api/auth/me", Some(&lab.wall)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_wall_display_limited_to_a_team_shows_only_its_monitors() {
    let lab = lab().await;
    let app = &lab.app;
    let team = app
        .post(
            "/api/teams",
            json!({
                "name": "Paris lobby",
                "members": [lab.wall_id],
                "grants": [{ "folder": "Paris", "level": "read" }]
            }),
            Some(&lab.admin),
        )
        .await;
    assert_eq!(team.status, StatusCode::CREATED, "{}", team.body);

    let me = app.get("/api/auth/me", Some(&lab.wall)).await;
    assert_eq!(me.body["access"]["scoped"], json!(true), "{}", me.body);
    let targets = app.get("/api/targets", Some(&lab.wall)).await;
    assert_eq!(targets.status, StatusCode::OK, "{}", targets.body);
    let names: Vec<&str> =
        targets.body.as_array().expect("liste").iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, ["nas"]);
    // Le reste du mur répond aussi sous le garde des équipes.
    for uri in ["/api/alerts", "/api/alerts/rules", "/api/instance/settings", "/api/wall/weather"] {
        let reply = app.get(uri, Some(&lab.wall)).await;
        assert_eq!(reply.status, StatusCode::OK, "{uri}: {}", reply.body);
    }
    // Et ce qui est hors du mur reste fermé, même ouvert aux équipes.
    assert_eq!(app.get("/api/topology", Some(&lab.wall)).await.status, StatusCode::FORBIDDEN);
}

/// Lit l'échéance enregistrée de la session du compte.
async fn expires_at(pool: &SqlitePool, user_id: i64) -> chrono::DateTime<Utc> {
    let raw: String = sqlx::query_scalar("SELECT expires_at FROM auth_sessions WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("session");
    chrono::DateTime::parse_from_rfc3339(&raw).expect("date").with_timezone(&Utc)
}

/// Vieillit la session du compte : vue il y a une heure, échéance dans un jour.
async fn age_session(pool: &SqlitePool, user_id: i64) {
    let at = |delta: Duration| (Utc::now() + delta).to_rfc3339_opts(SecondsFormat::Millis, true);
    sqlx::query("UPDATE auth_sessions SET last_seen_at = ?, expires_at = ? WHERE user_id = ?")
        .bind(at(Duration::hours(-1)))
        .bind(at(Duration::days(1)))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("session vieillie");
}

/// Un écran fixé au mur n'a personne pour retaper un mot de passe : tant qu'il
/// affiche le mur, sa session et son cookie repartent pour la durée entière.
/// `DUMBMONIT_SESSION_DAYS` devient son délai d'inactivité.
#[tokio::test]
async fn a_wall_session_is_renewed_while_the_wall_is_shown() {
    let lab = lab().await;
    let app = &lab.app;
    age_session(&lab.pool, lab.wall_id).await;

    let reply = app.get("/api/alerts", Some(&lab.wall)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let renewed = reply.set_cookie.expect("cookie renouvelé");
    assert!(renewed.contains(&max_age()), "{renewed}");
    assert!(renewed.contains(&lab.wall), "même session : {renewed}");
    let left = expires_at(&lab.pool, lab.wall_id).await - Utc::now();
    assert!(left > Duration::days(session::lifetime_days()) - Duration::hours(1), "{left}");

    // Dans la foulée, rien à renouveler : une écriture au plus toutes les
    // quelques minutes.
    let again = app.get("/api/alerts", Some(&lab.wall)).await;
    assert!(again.set_cookie.is_none(), "{:?}", again.set_cookie);

    // Un autre compte garde l'échéance fixée à sa connexion.
    let viewer = app.viewer_cookie(&lab.admin).await;
    let viewer_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'viewer'")
        .fetch_one(&lab.pool)
        .await
        .expect("lecteur");
    age_session(&lab.pool, viewer_id).await;
    let reply = app.get("/api/alerts", Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert!(reply.set_cookie.is_none(), "{:?}", reply.set_cookie);
    assert!(expires_at(&lab.pool, viewer_id).await - Utc::now() < Duration::days(2));
}

/// La déconnexion efface le cookie : le renouvellement ne le repose pas
/// derrière elle (le dernier `Set-Cookie` l'emporterait).
#[tokio::test]
async fn signing_out_is_not_undone_by_the_renewal() {
    let lab = lab().await;
    age_session(&lab.pool, lab.wall_id).await;
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header(axum::http::header::COOKIE, &lab.wall)
        .header("x-requested-with", "DumbMonit")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from("{}"))
        .unwrap();
    let response = tower::ServiceExt::oneshot(lab.app.router.clone(), request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let posed: Vec<&str> = response
        .headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect();
    assert_eq!(posed.len(), 1, "{posed:?}");
    assert!(posed[0].contains("Max-Age=0"), "{posed:?}");
    assert_eq!(lab.app.get("/api/auth/me", Some(&lab.wall)).await.status, StatusCode::UNAUTHORIZED);
}

/// Un écran qui quitte le rôle `wall` perd sa session : renouvelée sans fin,
/// elle donnerait au passant devant l'écran tout ce que permet le nouveau rôle.
#[tokio::test]
async fn the_session_ends_when_the_account_leaves_the_wall_role() {
    let lab = lab().await;
    let app = &lab.app;
    let reply = app
        .put(&format!("/api/users/{}", lab.wall_id), json!({ "role": "viewer" }), Some(&lab.admin))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(app.get("/api/auth/me", Some(&lab.wall)).await.status, StatusCode::UNAUTHORIZED);

    // L'inverse ne ferme rien : un compte qui devient écran mural perd des
    // droits, pas sa session.
    let viewer = app.login_as("lobby", WALL_PASSWORD).await.cookie();
    let reply = app
        .put(&format!("/api/users/{}", lab.wall_id), json!({ "role": "wall" }), Some(&lab.admin))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let me = app.get("/api/auth/me", Some(&viewer)).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.body);
    assert_eq!(me.body["role"], json!("wall"));
    assert_eq!(app.get("/api/users", Some(&viewer)).await.status, StatusCode::FORBIDDEN);
}

/// La politique « second facteur obligatoire » vise les administrateurs et les
/// opérateurs : un écran mural n'a personne pour taper un code.
#[tokio::test]
async fn the_two_factor_policy_does_not_hold_a_wall_display() {
    let lab = lab().await;
    let app = &lab.app;
    let saved =
        app.put("/api/auth/policy", json!({ "require_totp": true }), Some(&lab.admin)).await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);

    let wall = app.login_as("lobby", WALL_PASSWORD).await.cookie();
    for cookie in [&lab.wall, &wall] {
        let reply = app.get("/api/targets", Some(cookie)).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    }
    // Ce qui reste hors du mur répond le refus du mur, pas celui de la politique.
    let refused = app.get("/api/auth/totp", Some(&wall)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    assert_ne!(refused.body["code"], json!("totp_setup_required"), "{}", refused.body);
}

#[tokio::test]
async fn a_token_whose_owner_became_a_wall_display_opens_nothing() {
    let lab = lab().await;
    let app = &lab.app;
    let reply = app
        .post(
            "/api/users",
            json!({ "username": "boss", "role": "admin", "password": WALL_PASSWORD }),
            Some(&lab.admin),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let boss_id = reply.body["id"].as_i64().expect("id");
    let boss = app.login_as("boss", WALL_PASSWORD).await.cookie();
    let token = app.post("/api/tokens", json!({ "name": "t", "scope": "read" }), Some(&boss)).await;
    assert_eq!(token.status, StatusCode::CREATED, "{}", token.body);
    let secret = token.body["secret"].as_str().expect("secret").to_string();

    let with_token = move |secret: String| async move {
        let request = axum::http::Request::builder()
            .uri("/api/targets")
            .header(axum::http::header::AUTHORIZATION, format!("Bearer {secret}"))
            .body(axum::body::Body::empty())
            .unwrap();
        tower::ServiceExt::oneshot(app.router.clone(), request).await.unwrap().status()
    };
    assert_eq!(with_token(secret.clone()).await, StatusCode::OK);

    let reply = app
        .put(&format!("/api/users/{boss_id}"), json!({ "role": "wall" }), Some(&lab.admin))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(with_token(secret).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_wall_display_travels_with_the_backup() {
    let source_dir = tempfile::tempdir().unwrap();
    let source = db::open(&source_dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&source, "secret-de-test-suffisamment-long").await.expect("clé");
    users::insert(
        &source,
        NewUser {
            username: "lobby",
            display_name: "Lobby screen",
            role: Role::Wall,
            password_hash: None,
            oidc: None,
        },
    )
    .await
    .expect("compte")
    .expect("créé");

    let lot = collect(&source, &cipher, ExportOptions::default()).await.expect("export");
    let exported = lot.users.iter().find(|u| u.username == "lobby").expect("compte exporté");
    assert_eq!(exported.role, "wall");

    let target_dir = tempfile::tempdir().unwrap();
    let pool = db::open(&target_dir.path().join("dumbmonit.db")).await.expect("base");
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("clé");
    restore(&pool, &cipher, &lot, true).await.expect("restauration");
    let restored = users::by_username(&pool, "lobby").await.expect("lecture").expect("restauré");
    assert_eq!(restored.role, Role::Wall);
    assert_eq!(restored.display_name, "Lobby screen");
}
