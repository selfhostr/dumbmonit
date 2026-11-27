//! Canal des agents par WebSocket, de bout en bout : la route de l'interface,
//! une vraie mise à niveau, la session TLS de l'agent à l'intérieur.
//!
//! Le montage reproduit celui de l'agent : un pont local accepte les
//! connexions du client HTTP et ouvre pour chacune une WebSocket vers
//! `/api/agent/ws`, octet pour octet. Le client parle ensuite au canal des
//! agents comme en direct (autorité épinglée, certificat client) — le pont,
//! comme Cloudflare ou un mandataire, ne voit que du chiffré.

use crate::common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use dumbmonit_proto::{AGENT_WS_PATH, AGENT_WS_PROTOCOL};
use dumbmonit_server::state::{AppState, Inner};
use dumbmonit_server::{agent_tls, api, collectors, db, tsdb};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde_json::{Value, json};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::Role;

use common::TestApp;

const SERVER_NAME: &str = dumbmonit_proto::AGENT_TLS_SERVER_NAME;

struct Lab {
    app: TestApp,
    state: AppState,
    /// Port des agents (en direct).
    agents: SocketAddr,
    /// Port de l'interface, en clair pour le test : c'est la route qui compte,
    /// pas le TLS extérieur (celui de Cloudflare ou du mandataire).
    web: SocketAddr,
}

async fn lab_configured(configure: impl FnOnce(&mut dumbmonit_server::config::Config)) -> Lab {
    let dir = tempfile::tempdir().expect("répertoire temporaire");
    let mut config = common::base_config(dir.path());
    configure(&mut config);
    let pool = db::open(&config.database_path()).await.expect("base");
    let cipher = db::init_cipher(&pool, "secret-de-test-suffisamment-long").await.expect("chiffre");
    common::accept_enrolments(&pool).await;
    let mut registry = collectors::Registry::new();
    registry.register(Arc::new(collectors::AgentCollector::new(pool.clone())));
    let victoria = tsdb::Victoria::new(common::UNREACHABLE_VICTORIA.to_string()).expect("client");
    let sink = tsdb::spawn_writer(victoria.clone(), Duration::from_secs(60));
    let state = AppState::new(Inner { config, pool, cipher, victoria, sink, collectors: registry });
    let app = TestApp { router: api::router(state.clone()), _dir: dir };
    app.create_admin().await;

    agent_tls::init(&state).await.expect("canal des agents");
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("port");
    let agents = tcp.local_addr().expect("adresse");
    let listener = agent_tls::listen(state.clone(), tcp).expect("écoute");
    let router = agent_tls::router(state.clone());
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<agent_tls::AgentPeer>())
            .await
            .expect("canal des agents");
    });

    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("port");
    let web = tcp.local_addr().expect("adresse");
    let router = api::router(state.clone());
    tokio::spawn(async move {
        axum::serve(tcp, router.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .expect("interface");
    });
    Lab { app, state, agents, web }
}

async fn lab() -> (Lab, String) {
    let lab = lab_configured(|_| {}).await;
    let admin = lab.app.admin_cookie().await;
    (lab, admin)
}

async fn token(app: &TestApp, admin: &str, relay: bool) -> String {
    let reply = app
        .post("/api/agent/tokens", json!({ "reusable": true, "relay": relay }), Some(admin))
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "jeton : {}", reply.body);
    reply.body["secret"].as_str().expect("secret").to_string()
}

fn identity(machine_id: &str) -> Value {
    json!({
        "hostname": machine_id,
        "os": "linux",
        "agent_version": "0.18.0",
        "machine_id": machine_id,
        "commands_enabled": true,
        "binding_supported": true,
    })
}

fn batch(machine_id: &str) -> Value {
    json!({ "protocol": 1, "identity": identity(machine_id), "sent_at_ms": 0, "samples": [] })
}

async fn authority(lab: &Lab) -> Vec<u8> {
    sqlx::query_scalar("SELECT cert_der FROM agent_ca WHERE state = 'active'")
        .fetch_one(&lab.state.pool)
        .await
        .expect("autorité")
}

/// Demande de mise à niveau, telle que l'agent l'envoie.
fn ask_upgrade(lab: &Lab) -> reqwest::RequestBuilder {
    ask_upgrade_with(lab.web, AGENT_WS_PROTOCOL)
}

/// Demande de mise à niveau vers l'interface à `web`, pour ce sous-protocole.
fn ask_upgrade_with(web: SocketAddr, protocol: &str) -> reqwest::RequestBuilder {
    reqwest::Client::builder()
        .http1_only()
        .build()
        .expect("client")
        .get(format!("http://{web}{AGENT_WS_PATH}"))
        .header(reqwest::header::CONNECTION, "Upgrade")
        .header(reqwest::header::UPGRADE, "websocket")
        .header(reqwest::header::SEC_WEBSOCKET_VERSION, "13")
        .header(reqwest::header::SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ==")
        .header(reqwest::header::SEC_WEBSOCKET_PROTOCOL, protocol)
}

/// Ouvre la WebSocket des agents et la rend comme un flux d'octets.
async fn open_websocket(lab: &Lab) -> agent_tls::WsIo<reqwest::Upgraded> {
    let response = ask_upgrade(lab).send().await.expect("mise à niveau");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert_eq!(
        response.headers()[reqwest::header::SEC_WEBSOCKET_ACCEPT],
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=",
        "clé de la RFC 6455"
    );
    assert_eq!(response.headers()[reqwest::header::SEC_WEBSOCKET_PROTOCOL], AGENT_WS_PROTOCOL);
    let upgraded = response.upgrade().await.expect("flux");
    let config = agent_tls::websocket::config();
    let ws = WebSocketStream::from_raw_socket(upgraded, Role::Client, Some(config)).await;
    agent_tls::WsIo::new(ws)
}

/// Pont local, comme celui de l'agent : chaque connexion acceptée part dans
/// sa propre WebSocket vers l'interface.
async fn bridge(lab: &Lab) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("port");
    let addr = listener.local_addr().expect("adresse");
    let web = lab.web;
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let response =
                    ask_upgrade_with(web, AGENT_WS_PROTOCOL).send().await.expect("mise à niveau");
                assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
                let upgraded = response.upgrade().await.expect("flux");
                let config = agent_tls::websocket::config();
                let ws = WebSocketStream::from_raw_socket(upgraded, Role::Client, Some(config));
                let mut ws = agent_tls::WsIo::new(ws.await);
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut ws).await;
            });
        }
    });
    addr
}

/// Client du canal chiffré qui se connecte à `addr` (le pont), en vérifiant
/// toujours l'autorité du serveur, avec un certificat client s'il en a un.
fn client_at(addr: SocketAddr, ca: &[u8], identity: Option<(&[u8], &[u8])>) -> reqwest::Client {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(ca.to_vec())).expect("racine");
    let builder = rustls::ClientConfig::builder_with_provider(agent_tls::provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3")
        .with_root_certificates(roots);
    let config = match identity {
        Some((certificate, key)) => builder
            .with_client_auth_cert(
                vec![CertificateDer::from(certificate.to_vec())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.to_vec())),
            )
            .expect("certificat client"),
        None => builder.with_no_client_auth(),
    };
    reqwest::Client::builder()
        .tls_backend_preconfigured(config)
        .resolve(SERVER_NAME, addr)
        .build()
        .expect("client")
}

fn url_at(addr: SocketAddr, path: &str) -> String {
    format!("https://{SERVER_NAME}:{}{path}", addr.port())
}

struct Enrolled {
    certificate: Vec<u8>,
    key: Vec<u8>,
    secret: Option<String>,
}

/// Enrôle une machine à travers `through` (le pont, ou une passerelle).
async fn enrol_through(
    lab: &Lab,
    through: SocketAddr,
    enrolment: &str,
    machine_id: &str,
) -> (reqwest::Response, Vec<u8>) {
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).expect("clé");
    let ca = authority(lab).await;
    let response = client_at(through, &ca, None)
        .post(url_at(through, "/api/agent/tls/enroll"))
        .bearer_auth(enrolment)
        .json(&json!({ "identity": identity(machine_id), "public_key": BASE64.encode(key.public_key_raw()) }))
        .send()
        .await
        .expect("enrôlement");
    (response, key.serialize_der())
}

async fn enrolled_through(
    lab: &Lab,
    through: SocketAddr,
    enrolment: &str,
    machine_id: &str,
) -> Enrolled {
    let (response, key) = enrol_through(lab, through, enrolment, machine_id).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.expect("corps");
    Enrolled {
        certificate: BASE64.decode(body["certificate"].as_str().expect("certificat")).unwrap(),
        key,
        secret: body["agent_secret"].as_str().map(str::to_string),
    }
}

async fn push_through(
    lab: &Lab,
    through: SocketAddr,
    agent: &Enrolled,
    enrolment: &str,
    machine_id: &str,
) -> reqwest::Response {
    let ca = authority(lab).await;
    let mut request =
        client_at(through, &ca, Some((agent.certificate.as_slice(), agent.key.as_slice())))
            .post(url_at(through, "/api/ingest"))
            .bearer_auth(enrolment)
            .json(&batch(machine_id));
    if let Some(secret) = &agent.secret {
        request = request.header("x-dumbmonit-agent-secret", secret);
    }
    request.send().await.expect("lot")
}

async fn target_of(lab: &Lab, hostname: &str) -> i64 {
    sqlx::query_scalar("SELECT target_id FROM agent_hosts WHERE hostname = ?")
        .bind(hostname)
        .fetch_one(&lab.state.pool)
        .await
        .expect("machine")
}

async fn via_agent(lab: &Lab, target: i64) -> Option<i64> {
    sqlx::query_scalar("SELECT via_agent FROM targets WHERE id = ?")
        .bind(target)
        .fetch_one(&lab.state.pool)
        .await
        .expect("cible")
}

#[tokio::test]
async fn an_agent_enrols_and_reports_through_the_interface_websocket() {
    let (lab, admin) = lab().await;
    let through = bridge(&lab).await;
    let enrolment = token(&lab.app, &admin, false).await;

    // Enrôlement puis lot, sans jamais toucher le port des agents : la session
    // TLS de l'agent (autorité épinglée, certificat client) passe dans la
    // WebSocket de l'interface.
    let agent = enrolled_through(&lab, through, &enrolment, "id-cloudflare").await;
    assert!(agent.secret.is_some(), "première présentation : secret de liaison attribué");
    let response = push_through(&lab, through, &agent, &enrolment, "id-cloudflare").await;
    assert_eq!(response.status(), StatusCode::OK);
    let offer = response.headers().get(dumbmonit_proto::AGENT_TLS_HEADER).expect("annonce");
    let offer = dumbmonit_proto::TlsOffer::parse(offer.to_str().unwrap()).expect("annonce lisible");
    assert_eq!(offer.pins, lab.state.agent_tls().pins());

    // Le canal est bien le canal chiffré, échange post-quantique compris, et
    // la machine n'est derrière aucun relais.
    let group: Option<String> = sqlx::query_scalar("SELECT tls_group FROM agent_hosts")
        .fetch_one(&lab.state.pool)
        .await
        .unwrap();
    assert_eq!(group.as_deref(), Some(agent_tls::POST_QUANTUM_GROUP));
    let target = target_of(&lab, "id-cloudflare").await;
    assert_eq!(via_agent(&lab, target).await, None);

    // Le serveur constate le transport (l'agent ne le déclare pas) et la page
    // de la machine le montre.
    let reply = lab.app.get(&format!("/api/targets/{target}/agent"), Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["via_websocket"], true);

    // Le même agent, revenu en direct sur le port des agents : plus marqué.
    let response = push_through(&lab, lab.agents, &agent, &enrolment, "id-cloudflare").await;
    assert_eq!(response.status(), StatusCode::OK);
    let reply = lab.app.get(&format!("/api/targets/{target}/agent"), Some(&admin)).await;
    assert_eq!(reply.body["via_websocket"], false);
}

#[tokio::test]
async fn without_a_certificate_only_enrolment_answers_through_the_websocket() {
    let (lab, admin) = lab().await;
    let through = bridge(&lab).await;
    let enrolment = token(&lab.app, &admin, false).await;
    let ca = authority(&lab).await;
    let anonymous = client_at(through, &ca, None);

    let response = anonymous
        .post(url_at(through, "/api/ingest"))
        .bearer_auth(&enrolment)
        .json(&batch("id-x"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"], dumbmonit_proto::CERT_REFUSED);

    // Ni fichiers d'installation (servis en direct sur le port des agents),
    // ni rien d'autre.
    for path in ["/install.sh", "/install.ps1", "/api/health", "/"] {
        let response = anonymous.get(url_at(through, path)).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    let direct = client_at(lab.agents, &ca, None);
    let response = direct.get(url_at(lab.agents, "/install.sh")).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK, "en direct, le script reste servi");
}

#[tokio::test]
async fn bytes_that_are_not_the_agent_tls_session_reach_no_route() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (lab, _) = lab().await;
    let mut ws = open_websocket(&lab).await;
    // Une requête HTTP en clair à l'intérieur : pas une poignée de main TLS,
    // la connexion est fermée sans réponse.
    ws.write_all(b"POST /api/ingest HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
    ws.flush().await.unwrap();
    let mut reply = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(15), ws.read_to_end(&mut reply)).await;
    assert!(read.is_ok(), "la connexion est fermée, pas laissée pendante");
    assert!(!String::from_utf8_lossy(&reply).contains("HTTP/1.1"), "aucune réponse HTTP");
}

#[tokio::test]
async fn the_route_refuses_browsers_sessions_and_anything_but_the_agent_upgrade() {
    let (lab, admin) = lab().await;

    // Une page web : son navigateur envoie toujours `Origin`.
    let from_a_page = ask_upgrade(&lab).header("origin", "https://evil.example");
    assert_eq!(from_a_page.send().await.unwrap().status(), StatusCode::FORBIDDEN);
    // La session de l'interface n'ouvre rien ici.
    let response = ask_upgrade(&lab).header("cookie", admin.as_str()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // Une requête ordinaire, ou une WebSocket d'un autre protocole.
    let plain = reqwest::Client::new().get(format!("http://{}{AGENT_WS_PATH}", lab.web));
    assert_eq!(plain.send().await.unwrap().status(), StatusCode::UPGRADE_REQUIRED);
    let other = ask_upgrade_with(lab.web, "chat").send().await.unwrap();
    assert_eq!(other.status(), StatusCode::UPGRADE_REQUIRED, "un autre sous-protocole");
}

#[tokio::test]
async fn connections_from_one_address_are_bounded_and_the_route_can_be_closed() {
    let lab = lab_configured(|config| config.agent_ws_max_per_ip = 1).await;
    let held = open_websocket(&lab).await;
    let response = ask_upgrade(&lab).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let ip = "127.0.0.1".parse().unwrap();
    assert_eq!(lab.state.agent_tls().ws_limits().open_from(ip), 1);
    // La place se libère avec la connexion.
    drop(held);
    let mut freed = false;
    for _ in 0..50 {
        if lab.state.agent_tls().ws_limits().open_from(ip) == 0 {
            freed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(freed, "place rendue à la fermeture");

    let closed = lab_configured(|config| config.agent_websocket = false).await;
    let response = ask_upgrade(&closed).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Connexion multiplexée d'un relais à travers `through` (le pont), comme
/// celle d'un relais à jour passé par la WebSocket : un flux yamux par
/// connexion acceptée sur la passerelle rendue.
async fn mux_gateway_through(lab: &Lab, through: SocketAddr, relay: &Enrolled) -> SocketAddr {
    use std::task::Poll;

    use tokio::sync::{mpsc, oneshot};
    use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("port");
    let addr = listener.local_addr().expect("adresse");
    let ca = authority(lab).await;
    let response =
        client_at(through, &ca, Some((relay.certificate.as_slice(), relay.key.as_slice())))
            .get(url_at(through, dumbmonit_proto::RELAY_TUNNEL_PATH))
            .header(reqwest::header::CONNECTION, "upgrade")
            .header(reqwest::header::UPGRADE, dumbmonit_proto::RELAY_MUX_PROTOCOL)
            .send()
            .await
            .expect("connexion multiplexée");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    let upgraded = response.upgrade().await.expect("mise à niveau");
    let socket = TokioAsyncReadCompatExt::compat(upgraded);
    let mut connection =
        yamux::Connection::new(socket, yamux::Config::default(), yamux::Mode::Client);
    let (asks, mut asked) = mpsc::channel::<oneshot::Sender<yamux::Stream>>(16);
    tokio::spawn(async move {
        let mut waiting = std::collections::VecDeque::new();
        std::future::poll_fn(|cx| {
            while let Poll::Ready(Some(reply)) = asked.poll_recv(cx) {
                waiting.push_back(reply);
            }
            while !waiting.is_empty() {
                match connection.poll_new_outbound(cx) {
                    Poll::Ready(Ok(stream)) => {
                        let _ = waiting.pop_front().expect("demande").send(stream);
                    }
                    Poll::Ready(Err(_)) => return Poll::Ready(()),
                    Poll::Pending => break,
                }
            }
            loop {
                match connection.poll_next_inbound(cx) {
                    Poll::Ready(Some(Ok(_))) => {}
                    Poll::Ready(_) => return Poll::Ready(()),
                    Poll::Pending => return Poll::Pending,
                }
            }
        })
        .await;
    });
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (reply, stream) = oneshot::channel();
            asks.send(reply).await.expect("connexion multiplexée menée");
            let stream = stream.await.expect("flux");
            tokio::spawn(async move {
                let mut stream = FuturesAsyncReadCompatExt::compat(stream);
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
            });
        }
    });
    addr
}

#[tokio::test]
async fn a_relay_multiplexes_its_site_through_the_websocket() {
    let (lab, admin) = lab().await;
    let through = bridge(&lab).await;
    // Le relais s'enrôle lui-même par la WebSocket, puis y ouvre sa connexion
    // multiplexée : les agents du site passent dedans, chacun avec sa propre
    // session TLS, dans celle du relais, dans la WebSocket.
    let relay_enrolment = token(&lab.app, &admin, true).await;
    let relay = enrolled_through(&lab, through, &relay_enrolment, "id-relais").await;
    let relay_id = target_of(&lab, "id-relais").await;
    let site_gateway = mux_gateway_through(&lab, through, &relay).await;

    let enrolment = token(&lab.app, &admin, false).await;
    let agent = enrolled_through(&lab, site_gateway, &enrolment, "id-site").await;
    let site = target_of(&lab, "id-site").await;
    assert_eq!(via_agent(&lab, site).await, Some(relay_id), "le relais est le parent");
    let response = push_through(&lab, site_gateway, &agent, &enrolment, "id-site").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(lab.state.agent_tls().tunnels_of(relay_id) >= 1);
}

#[tokio::test]
async fn behind_a_proxy_the_install_command_goes_through_the_websocket() {
    let (lab, admin) = lab().await;
    let query = "/api/agent/install?base_url=https://monit.example.com&transport=websocket";
    let reply = lab.app.get(query, Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["transport"], "websocket");
    assert_eq!(reply.body["base_url"], "https://monit.example.com");
    let linux = reply.body["linux_template"].as_str().unwrap();
    assert!(linux.contains(" --url=https://monit.example.com"), "{linux}");
    assert!(linux.contains(" --transport=websocket"), "{linux}");
    assert!(linux.contains(" --ca-pin="), "l'autorité reste épinglée : {linux}");
    let windows = reply.body["windows_template"].as_str().unwrap();
    assert!(windows.contains(" -Transport websocket"), "{windows}");

    // Même un jeton de relais (port des agents seul) passe alors par l'adresse
    // de l'interface : le port des agents n'y est pas joignable.
    let created = lab
        .app
        .post(
            "/api/agent/tokens",
            json!({
                "base_url": "https://monit.example.com",
                "relay": true,
                "transport": "websocket",
            }),
            Some(&admin),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["transport"], "websocket");
    let linux = created.body["install_linux"].as_str().unwrap();
    assert!(linux.starts_with("curl -sSL https://monit.example.com/install.sh"), "{linux}");
    assert!(linux.contains(" --transport=websocket"), "{linux}");

    // Sans demande ni intermédiaire constaté : rien d'imposé.
    let reply = lab.app.get("/api/agent/install?base_url=https://nas.lan:8080", Some(&admin)).await;
    assert_eq!(reply.body["transport"], Value::Null);
    assert!(!reply.body["linux_template"].as_str().unwrap().contains("--transport"));
    // `direct` n'impose rien non plus, même demandé.
    let query = "/api/agent/install?base_url=https://monit.example.com&transport=direct";
    let reply = lab.app.get(query, Some(&admin)).await;
    assert_eq!(reply.body["transport"], Value::Null);
}

#[tokio::test]
async fn the_websocket_transport_is_never_offered_when_the_route_is_closed() {
    let lab = lab_configured(|config| config.agent_websocket = false).await;
    let admin = lab.app.admin_cookie().await;
    let query = "/api/agent/install?base_url=https://monit.example.com&transport=websocket";
    let reply = lab.app.get(query, Some(&admin)).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["transport"], Value::Null, "la route fermée n'est jamais proposée");
    assert!(!reply.body["linux_template"].as_str().unwrap().contains("--transport"));
}
