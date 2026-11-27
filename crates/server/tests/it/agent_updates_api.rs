//! Mise à jour automatique des agents, par l'API réelle : l'offre jointe à
//! l'accusé de réception, ses conditions (empreinte, réglages, fenêtre,
//! demande explicite, échec rapporté), le téléchargement par le canal des
//! agents et la vue de l'interface.

use crate::common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use common::{Reply, TestApp};

const SECRET_HEADER: &str = "x-dumbmonit-agent-secret";
const KEY_HEADER: &str = "x-dumbmonit-agent-key";
const BINARY: &[u8] = b"un faux binaire d'agent, version de ce serveur";
const FILE: &str = "dumbmonit-agent-linux-x86_64";

struct Fleet {
    app: TestApp,
    _agents: tempfile::TempDir,
    admin: String,
    enrolment: String,
    sha256: String,
}

/// Instance qui livre un binaire Linux signé, un administrateur et un jeton.
async fn fleet() -> Fleet {
    fleet_with(true).await
}

/// La même, avec un binaire signé ou non (build sans clé de signature).
async fn fleet_with(signed: bool) -> Fleet {
    let agents = tempfile::tempdir().expect("répertoire des agents");
    std::fs::write(agents.path().join(FILE), BINARY).unwrap();
    if signed {
        std::fs::write(agents.path().join(format!("{FILE}.sig")), [7u8; 64]).unwrap();
    }
    let dir = agents.path().to_path_buf();
    let app = common::setup_with(move |config| config.agent_dir = dir).await;
    app.create_admin().await;
    let admin = app.admin_cookie().await;
    let token = json!({ "name": "parc", "reusable": true });
    let reply = app.post("/api/agent/tokens", token, Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let enrolment = reply.body["secret"].as_str().unwrap().to_string();
    let sha256 = hex::encode(Sha256::digest(BINARY));
    Fleet { app, _agents: agents, admin, enrolment, sha256 }
}

async fn send(app: &TestApp, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = app.router.clone().oneshot(request).await.expect("réponse");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("corps").to_bytes();
    (status, bytes.to_vec())
}

/// Un lot vide ; `self_update` : ce que l'agent dit de son binaire.
async fn push(fleet: &Fleet, secret: Option<&str>, self_update: Value) -> Reply {
    push_machine(fleet, "nas", secret, self_update).await
}

/// Le même lot, pour la machine `machine` (nom d'hôte ; identifiant `id-<nom>`).
async fn push_machine(
    fleet: &Fleet,
    machine: &str,
    secret: Option<&str>,
    self_update: Value,
) -> Reply {
    // 0.20.1 : la première version qui connaît la clé de publication actuelle.
    push_version(fleet, machine, "0.20.1", secret, self_update).await
}

/// Le même lot, d'un agent de version `version`.
async fn push_version(
    fleet: &Fleet,
    machine: &str,
    version: &str,
    secret: Option<&str>,
    self_update: Value,
) -> Reply {
    let mut identity = json!({
        "hostname": machine,
        "os": "linux",
        "arch": "x86_64",
        "agent_version": version,
        "machine_id": format!("id-{machine}"),
        "binding_supported": true,
    });
    if !self_update.is_null() {
        identity["self_update"] = self_update;
    }
    let batch = json!({ "protocol": 1, "identity": identity, "sent_at_ms": 0, "samples": [] });
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/ingest")
        .header(header::AUTHORIZATION, format!("Bearer {}", fleet.enrolment))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(secret) = secret {
        builder = builder.header(SECRET_HEADER, secret);
    }
    let request = builder.body(Body::from(batch.to_string())).unwrap();
    let (status, bytes) = send(&fleet.app, request).await;
    Reply {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        set_cookie: None,
        location: None,
    }
}

fn running(sha: &str) -> Value {
    json!({ "sha256": sha, "platform": "linux-x86_64", "ability": "ready", "signing_keys": true })
}

/// Enrôle la machine et rend (secret de liaison, cible).
async fn enrolled(fleet: &Fleet) -> (String, i64) {
    enrolled_machine(fleet, "nas").await
}

async fn enrolled_machine(fleet: &Fleet, machine: &str) -> (String, i64) {
    let reply = push_machine(fleet, machine, None, Value::Null).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    (
        reply.body["agent_secret"].as_str().unwrap().to_string(),
        reply.body["target_id"].as_i64().unwrap(),
    )
}

fn agent_status(view: &Reply, id: i64) -> Value {
    view.body["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["id"] == id)
        .cloned()
        .expect("agent listé")
}

#[tokio::test]
async fn an_outdated_agent_is_offered_the_signed_binary_of_this_build() {
    let fleet = fleet().await;
    let (secret, target) = enrolled(&fleet).await;
    let old = "0".repeat(64);

    let reply = push(&fleet, Some(&secret), running(&old)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let offer = &reply.body["agent_update"];
    assert_eq!(offer["file"], FILE);
    assert_eq!(offer["sha256"], fleet.sha256);
    assert_eq!(offer["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(offer["size"], BINARY.len());
    let signature = base64::engine::general_purpose::STANDARD.encode([7u8; 64]);
    assert_eq!(offer["signature"], signature);
    assert_eq!(offer["allow_unsigned"], false);
    assert_eq!(offer["force"], false);

    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(view.status, StatusCode::OK, "{}", view.body);
    assert_eq!(agent_status(&view, target)["status"], "updating");
    assert_eq!(view.body["binaries"][0]["signed"], true);

    // Une fois le binaire en place : plus d'offre, « à jour ».
    let reply = push(&fleet, Some(&secret), running(&fleet.sha256)).await;
    assert!(reply.body.get("agent_update").is_none(), "{}", reply.body);
    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(agent_status(&view, target)["status"], "up_to_date");
}

#[tokio::test]
async fn older_agents_and_containers_are_never_offered_anything() {
    let fleet = fleet().await;
    let (secret, target) = enrolled(&fleet).await;

    // Agent antérieur : il ne dit rien de son binaire.
    let reply = push(&fleet, Some(&secret), Value::Null).await;
    assert!(reply.body.get("agent_update").is_none());
    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(agent_status(&view, target)["status"], "legacy");

    let container = json!({ "sha256": "0".repeat(64), "platform": "linux-x86_64",
                            "ability": "container" });
    let reply = push(&fleet, Some(&secret), container).await;
    assert!(reply.body.get("agent_update").is_none());
    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(agent_status(&view, target)["status"], "container");
}

#[tokio::test]
async fn the_manual_window_waits_for_update_now() {
    let fleet = fleet().await;
    let (secret, target) = enrolled(&fleet).await;
    let manual = json!({ "window": "manual" });
    let reply = fleet.app.put("/api/agent/updates", manual, Some(&fleet.admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["settings"]["window"], "manual");

    let old = "0".repeat(64);
    let reply = push(&fleet, Some(&secret), running(&old)).await;
    assert!(reply.body.get("agent_update").is_none(), "fenêtre manuelle : rien");

    let now = format!("/api/agent/updates/{target}/now");
    let reply = fleet.app.post(&now, json!({}), Some(&fleet.admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let reply = push(&fleet, Some(&secret), running(&old)).await;
    assert_eq!(reply.body["agent_update"]["force"], true, "{}", reply.body);
}

#[tokio::test]
async fn a_rolled_back_version_is_not_offered_again_by_itself() {
    let fleet = fleet().await;
    let (secret, target) = enrolled(&fleet).await;
    let old = "0".repeat(64);
    let mut info = running(&old);
    info["last"] = json!({
        "outcome": "rolled_back",
        "from_version": "0.0.1",
        "to_version": env!("CARGO_PKG_VERSION"),
        "to_sha256": fleet.sha256,
        "reason": "the new agent had no batch accepted by the server within 120 s",
        "at_ms": chrono::Utc::now().timestamp_millis(),
    });

    let reply = push(&fleet, Some(&secret), info).await;
    assert!(reply.body.get("agent_update").is_none(), "{}", reply.body);
    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    let agent = agent_status(&view, target);
    assert_eq!(agent["status"], "failed");
    assert!(agent["reason"].as_str().unwrap().contains("120 s"));
    assert_eq!(agent["last"]["outcome"], "rolled_back");
}

#[tokio::test]
async fn the_per_agent_switch_and_the_global_one() {
    let fleet = fleet().await;
    let (secret, target) = enrolled(&fleet).await;
    let old = "0".repeat(64);
    let agent = format!("/api/agent/updates/{target}");
    assert!(push(&fleet, Some(&secret), running(&old)).await.body.get("agent_update").is_some());

    let reply = fleet.app.put(&agent, json!({ "enabled": false }), Some(&fleet.admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(agent_status(&reply, target)["status"], "disabled");
    assert!(push(&fleet, Some(&secret), running(&old)).await.body.get("agent_update").is_none());

    // De retour au réglage global, coupé lui aussi.
    fleet.app.put(&agent, json!({ "enabled": null }), Some(&fleet.admin)).await;
    fleet.app.put("/api/agent/updates", json!({ "enabled": false }), Some(&fleet.admin)).await;
    assert!(push(&fleet, Some(&secret), running(&old)).await.body.get("agent_update").is_none());
    // Forcée pour la machine : elle passe.
    fleet.app.put(&agent, json!({ "enabled": true, "canary": true }), Some(&fleet.admin)).await;
    let reply = push(&fleet, Some(&secret), running(&old)).await;
    assert_eq!(reply.body["agent_update"]["sha256"], fleet.sha256, "{}", reply.body);

    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(agent_status(&view, target)["canary"], true);
    assert_eq!(view.body["rollout"]["canaries"], 1);
    let missing = fleet.app.put("/api/agent/updates/999999", json!({}), Some(&fleet.admin)).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    let night = json!({ "window": "nuit" });
    let bad = fleet.app.put("/api/agent/updates", night, Some(&fleet.admin)).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_binary_is_downloaded_through_the_agent_channel_by_a_bound_machine_only() {
    let fleet = fleet().await;
    let (secret, _) = enrolled(&fleet).await;
    let download = |secret: Option<&str>| {
        let mut builder = Request::builder()
            .uri(format!("/api/agent/update/download/{FILE}"))
            .header(header::AUTHORIZATION, format!("Bearer {}", fleet.enrolment))
            .header(KEY_HEADER, "id-nas");
        if let Some(secret) = secret {
            builder = builder.header(SECRET_HEADER, secret);
        }
        builder.body(Body::empty()).unwrap()
    };

    let (status, bytes) = send(&fleet.app, download(Some(&secret))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, BINARY);
    // Sans le secret de liaison, une voisine porteuse du jeton n'obtient rien.
    let (status, _) = send(&fleet.app, download(None)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // La signature est aussi servie à côté du binaire, pour qui vérifie à la main.
    let request = Request::builder().uri(format!("/download/{FILE}.sig")).body(Body::empty());
    let (status, bytes) = send(&fleet.app, request.unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, [7u8; 64]);
}

#[tokio::test]
async fn the_update_settings_are_for_administrators_in_a_session() {
    let fleet = fleet().await;
    let viewer = fleet.app.viewer_cookie(&fleet.admin).await;
    let off = json!({ "enabled": false });
    let reply = fleet.app.put("/api/agent/updates", off, Some(&viewer)).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.body);
    let reply = fleet.app.get("/api/agent/updates", None).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_unsigned_build_blocks_the_updates_and_the_view_says_so() {
    // Un build fait sans clé de signature : le binaire livré n'a pas de `.sig`.
    let fleet = fleet_with(false).await;
    let (secret, target) = enrolled(&fleet).await;
    let reply = push(&fleet, Some(&secret), running(&"a".repeat(64))).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert!(reply.body.get("agent_update").is_none(), "rien n'est proposé : {}", reply.body);

    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(view.status, StatusCode::OK, "{}", view.body);
    assert_eq!(view.body["blocked_unsigned"], true);
    assert_eq!(agent_status(&view, target)["status"], "unsigned");

    // Accepter les builds non signés (développement) débloque.
    let reply = fleet
        .app
        .put("/api/agent/updates", json!({ "allow_unsigned": true }), Some(&fleet.admin))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["blocked_unsigned"], false);
}

#[tokio::test]
async fn by_default_there_is_no_wave_and_pause_and_resume_hold_the_offers() {
    let fleet = fleet().await;
    let (secret, target) = enrolled(&fleet).await;
    let old = "0".repeat(64);

    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(view.body["rollout"]["waves"], json!([]), "{}", view.body);
    assert_eq!(view.body["rollout"]["policy"]["waves"], json!([]));
    assert_eq!(view.body["rollout"]["paused"], false);

    let pause = "/api/agent/updates/rollout/pause";
    let paused = fleet.app.post(pause, json!({}), Some(&fleet.admin)).await;
    assert_eq!(paused.status, StatusCode::OK, "{}", paused.body);
    assert_eq!(paused.body["rollout"]["paused"], true);
    let reply = push(&fleet, Some(&secret), running(&old)).await;
    assert!(reply.body.get("agent_update").is_none(), "en pause : rien {}", reply.body);
    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(agent_status(&view, target)["status"], "paused");

    // « Update now » passe outre la pause.
    let now = format!("/api/agent/updates/{target}/now");
    fleet.app.post(&now, json!({}), Some(&fleet.admin)).await;
    let reply = push(&fleet, Some(&secret), running(&old)).await;
    assert_eq!(reply.body["agent_update"]["force"], true, "{}", reply.body);
    // Mise à jour faite : la demande est close, la pause vaut de nouveau.
    push(&fleet, Some(&secret), running(&fleet.sha256)).await;
    let reply = push(&fleet, Some(&secret), running(&old)).await;
    assert!(reply.body.get("agent_update").is_none(), "{}", reply.body);

    let resume = "/api/agent/updates/rollout/resume";
    let resumed = fleet.app.post(resume, json!({}), Some(&fleet.admin)).await;
    assert_eq!(resumed.body["rollout"]["paused"], false);
    let reply = push(&fleet, Some(&secret), running(&old)).await;
    assert!(reply.body.get("agent_update").is_some(), "{}", reply.body);
}

#[tokio::test]
async fn the_wave_policy_is_validated_and_kept() {
    let fleet = fleet().await;
    for bad in [
        json!({ "waves": [{ "by": "percent", "value": 0 }] }),
        json!({ "waves": [{ "by": "site", "value": " " }] }),
        json!({ "waves": [{ "by": "moon", "value": 1 }] }),
        json!({ "max_failure_percent": 101 }),
        json!({ "delay_minutes": 10081 }),
    ] {
        let reply = fleet.app.put("/api/agent/updates/rollout", bad, Some(&fleet.admin)).await;
        assert!(reply.status.is_client_error(), "{}", reply.body);
    }
    let good = json!({
        "waves": [
            { "by": "count", "value": 1 },
            { "by": "percent", "value": 50 },
            { "by": "tag", "key": "env", "value": "prod" }
        ],
        "delay_minutes": 15,
        "max_failure_percent": 10
    });
    let reply = fleet.app.put("/api/agent/updates/rollout", good, Some(&fleet.admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let policy = &reply.body["rollout"]["policy"];
    assert_eq!(policy["waves"].as_array().unwrap().len(), 3);
    assert_eq!(policy["delay_minutes"], 15);
    assert_eq!(policy["max_failure_percent"], 10);
    // Les champs absents sont inchangés ; une liste vide revient au déploiement historique.
    let reply = fleet
        .app
        .put("/api/agent/updates/rollout", json!({ "waves": [] }), Some(&fleet.admin))
        .await;
    assert_eq!(reply.body["rollout"]["policy"]["waves"], json!([]));
    assert_eq!(reply.body["rollout"]["policy"]["delay_minutes"], 15);

    let viewer = fleet.app.viewer_cookie(&fleet.admin).await;
    let denied = fleet.app.post("/api/agent/updates/rollout/pause", json!({}), Some(&viewer)).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN, "{}", denied.body);
}

#[tokio::test]
async fn a_later_wave_waits_and_a_failure_halts_the_rest_until_resumed() {
    let fleet = fleet().await;
    let old = "0".repeat(64);
    let mut machines = Vec::new();
    for name in ["alpha", "beta"] {
        let (secret, id) = enrolled_machine(&fleet, name).await;
        // Chacune dit son binaire : elle devient candidate au déploiement.
        push_machine(&fleet, name, Some(&secret), running(&old)).await;
        machines.push((name, secret, id));
    }
    let waves = json!({ "waves": [{ "by": "count", "value": 1 }], "delay_minutes": 60 });
    let reply = fleet.app.put("/api/agent/updates/rollout", waves, Some(&fleet.admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);

    let find = |view: &Reply, wave: i64| {
        machines
            .iter()
            .find(|(_, _, id)| agent_status(view, *id)["wave"] == wave)
            .cloned()
            .expect("une machine dans cette vague")
    };
    let (lead, lead_secret, lead_id) = find(&reply, 1);
    let (follow, follow_secret, follow_id) = find(&reply, 2);
    assert_eq!(reply.body["rollout"]["current_wave"], 1);

    // La vague suivante attend : rien pour elle, tout pour la première.
    let held = push_machine(&fleet, follow, Some(&follow_secret), running(&old)).await;
    assert!(held.body.get("agent_update").is_none(), "{}", held.body);
    let first = push_machine(&fleet, lead, Some(&lead_secret), running(&old)).await;
    assert!(first.body.get("agent_update").is_some(), "{}", first.body);
    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(agent_status(&view, follow_id)["status"], "waiting_wave");

    // La première vague a fini : la suivante n'attend plus que le délai.
    push_machine(&fleet, lead, Some(&lead_secret), running(&fleet.sha256)).await;
    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert!(view.body["rollout"]["next_wave_at"].is_string(), "{}", view.body);
    assert_eq!(view.body["rollout"]["waves"][0]["state"], "done");
    let held = push_machine(&fleet, follow, Some(&follow_secret), running(&old)).await;
    assert!(held.body.get("agent_update").is_none(), "{}", held.body);
    let instant = json!({ "delay_minutes": 0 });
    fleet.app.put("/api/agent/updates/rollout", instant, Some(&fleet.admin)).await;
    let open = push_machine(&fleet, follow, Some(&follow_secret), running(&old)).await;
    assert!(open.body.get("agent_update").is_some(), "{}", open.body);

    // Nouvelle version : le canari échoue, la propagation s'arrête.
    let mut failed = running(&old);
    failed["last"] = json!({
        "outcome": "rolled_back",
        "from_version": "0.0.1",
        "to_version": env!("CARGO_PKG_VERSION"),
        "to_sha256": fleet.sha256,
        "reason": "boom",
        "at_ms": chrono::Utc::now().timestamp_millis(),
    });
    push_machine(&fleet, lead, Some(&lead_secret), failed).await;
    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(view.body["rollout"]["halted"]["wave"], 1, "{}", view.body);
    assert_eq!(agent_status(&view, lead_id)["status"], "failed");
    assert_eq!(agent_status(&view, follow_id)["status"], "halted");
    let other = running(&"2".repeat(64));
    let stopped = push_machine(&fleet, follow, Some(&follow_secret), other.clone()).await;
    assert!(stopped.body.get("agent_update").is_none(), "{}", stopped.body);

    // Reprise : l'échec est pardonné, la suite repart.
    let path = "/api/agent/updates/rollout/resume";
    let reply = fleet.app.post(path, json!({}), Some(&fleet.admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert!(reply.body["rollout"]["halted"].is_null(), "{}", reply.body);
    let go = push_machine(&fleet, follow, Some(&follow_secret), other).await;
    assert!(go.body.get("agent_update").is_some(), "{}", go.body);
}

#[tokio::test]
async fn an_agent_before_the_current_key_gets_the_build_unsigned_only_when_allowed() {
    // Agents 0.15.0 à 0.20.0 : seule la première clé, qui n'a jamais rien signé.
    let fleet = fleet().await;
    let (secret, target) = enrolled(&fleet).await;
    let old = "0".repeat(64);
    let reply = push_version(&fleet, "nas", "0.15.0", Some(&secret), running(&old)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert!(reply.body.get("agent_update").is_none(), "rien sans le réglage : {}", reply.body);
    let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
    assert_eq!(agent_status(&view, target)["status"], "unknown_key");

    let allow = json!({ "allow_unsigned": true });
    let reply = fleet.app.put("/api/agent/updates", allow, Some(&fleet.admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let reply = push_version(&fleet, "nas", "0.15.0", Some(&secret), running(&old)).await;
    let offer = &reply.body["agent_update"];
    assert_eq!(offer["sha256"], fleet.sha256, "{}", reply.body);
    assert!(offer.get("signature").is_none(), "offre sans signature : {}", reply.body);
    assert_eq!(offer["allow_unsigned"], true);

    // Un agent qui connaît la clé reçoit toujours la signature.
    let reply = push(&fleet, Some(&secret), running(&old)).await;
    assert!(reply.body["agent_update"]["signature"].is_string(), "{}", reply.body);
}

#[tokio::test]
async fn an_old_windows_agent_is_asked_for_a_reinstall_and_offered_nothing() {
    // Jusqu'à 0.20.1, l'agent Windows échoue toujours à synchroniser sa copie.
    let fleet = fleet().await;
    let exe = fleet._agents.path().join("dumbmonit-agent-windows-x86_64.exe");
    std::fs::write(&exe, b"un faux agent Windows").unwrap();
    std::fs::write(exe.with_extension("exe.sig"), [7u8; 64]).unwrap();
    let (secret, target) = enrolled(&fleet).await;
    let windows = json!({ "sha256": "0".repeat(64), "platform": "windows-x86_64",
                          "ability": "ready", "signing_keys": true });
    let allow = json!({ "allow_unsigned": true });
    let reply = fleet.app.put("/api/agent/updates", allow, Some(&fleet.admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    // « Update now » n'y change rien : l'agent échouerait au même endroit.
    let now = format!("/api/agent/updates/{target}/now");
    fleet.app.post(&now, json!({}), Some(&fleet.admin)).await;

    for version in ["0.14.3", "0.15.0", "0.20.1"] {
        let reply = push_version(&fleet, "nas", version, Some(&secret), windows.clone()).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        assert!(reply.body.get("agent_update").is_none(), "{version} : {}", reply.body);
        let view = fleet.app.get("/api/agent/updates", Some(&fleet.admin)).await;
        assert_eq!(agent_status(&view, target)["status"], "reinstall", "{version}");
    }
    // Avec le correctif, l'offre revient.
    let reply =
        push_version(&fleet, "nas", env!("CARGO_PKG_VERSION"), Some(&secret), windows).await;
    let offer = &reply.body["agent_update"];
    assert_eq!(offer["file"], "dumbmonit-agent-windows-x86_64.exe", "{}", reply.body);
    assert!(offer["signature"].is_string(), "{}", reply.body);
}
