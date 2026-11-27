//! Tests de la boucle de vie de l'agent : jeux d'essai partagés.

use super::*;
use crate::config::Config;

mod channel;
mod exchange;
mod relay;
mod settings;

fn config() -> Config {
    Config {
        server_url: "http://127.0.0.1:1".into(),
        token: "dmon_test".into(),
        interval: Duration::from_secs(30),
        hostname: Some("machine-de-test".into()),
        services: Vec::new(),
        tags: std::collections::BTreeMap::new(),
        modules: None,
        docker: false,
        docker_socket: std::path::PathBuf::from("/inexistant.sock"),
        docker_update_check: false,
        docker_max_containers: 200,
        commands: false,
        actions: crate::remediation::ActionsConfig::default(),
        scripts: crate::scripts::ScriptsConfig::default(),
        relay: false,
        site: None,
        gateway: crate::gateway::GatewayConfig::default(),
        probe: crate::collect::ProbeConfig::default(),
        system_health: crate::collect::system_health::SystemHealthConfig::default(),
        plakar: crate::collect::plakar::PlakarConfig::default(),
        sensors: false,
        smart: crate::collect::smart::SmartConfig::default(),
        zfs: crate::collect::zfs::ZfsConfig::default(),
        libvirt: crate::collect::libvirt::LibvirtConfig::default(),
        wireguard: crate::collect::wireguard::WireguardConfig::default(),
        ipsec: crate::collect::ipsec::IpsecConfig::default(),
        speedtest: crate::collect::speedtest::SpeedtestConfig::default(),
        inventory: false,
        self_update: true,
        backup_repos: crate::collect::backup_repos::BackupReposConfig::default(),
        databases: crate::collect::databases::DatabasesConfig::default(),
        perf_counters: crate::collect::perf_counters::PerfCountersConfig::default(),
        max_buffered_samples: 1_000,
        buffer_max_mb: 0,
        secret_path: std::path::PathBuf::from("/inexistant/agent-secret"),
        tls_url: None,
        ca_pin: None,
        server_pin: None,
        plain_server_url: false,
        fallback_servers: Vec::new(),
        transport: crate::ws::TransportMode::Auto,
        log_level: tracing::Level::INFO,
        proxy: crate::proxy::ProxyConfig::default(),
    }
}

fn setting(enabled: bool, site: Option<&str>) -> Option<RelaySetting> {
    Some(RelaySetting {
        enabled,
        site: site.map(str::to_string),
        gateway_port: None,
        gateway_host: None,
    })
}

// ------------------------------------------------ contre un faux serveur

const ACK: &str = r#"{"target_id":3,"accepted":1,"interval_secs":60,
    "agent_secret":"dmab_attribue","bound":true}"#;

/// Agent pointé sur le faux serveur, avec un secret de liaison jetable.
fn agent_for(url: &str, dir: &tempfile::TempDir) -> Agent {
    let mut config = config();
    config.server_url = url.to_string();
    config.secret_path = dir.path().join("agent-secret");
    Agent::new(config).expect("agent")
}

const PIN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Un port local fermé : rien n'y répond.
fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Canal chiffré rangé d'un agent enrôlé : un vrai certificat et sa clé.
fn enrolled_state(url: &str) -> TlsState {
    use base64::Engine as _;
    let engine = base64::engine::general_purpose::STANDARD;
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let params = rcgen::CertificateParams::new(vec!["agent".to_string()]).unwrap();
    let cert = params.self_signed(&key).unwrap();
    let mut state = TlsState::new(url.into(), vec![PIN.to_string()]);
    state.certificate = Some(engine.encode(cert.der()));
    state.key = Some(engine.encode(key.serialize_der()));
    state
}

/// Le faux canal accepte les lots ; tout le reste (commandes…) est inconnu.
fn channel_ingest(request: &crate::testsupport::Req) -> crate::testsupport::Resp {
    match (request.method.as_str(), request.path.as_str()) {
        (_, "/api/ingest") => (200, ACK.to_string()),
        // L'essai d'un transport : une réponse sans corps suffit.
        ("HEAD", _) => (404, String::new()),
        _ => (404, r#"{"error":"Not found."}"#.to_string()),
    }
}

/// Agent installé avant le tout-HTTPS : `server_url` en clair, hors boucle.
fn plain_config(dir: &tempfile::TempDir) -> Config {
    let mut config = config();
    config.server_url = "http://192.168.254.240:8443".into();
    config.plain_server_url = true;
    config.secret_path = dir.path().join("agent-secret");
    config
}

/// Agent relais pointé sur le faux serveur, sans module ni commande.
fn relay_agent_for(url: &str, dir: &tempfile::TempDir) -> Agent {
    let mut config = config();
    config.server_url = url.to_string();
    config.secret_path = dir.path().join("agent-secret");
    config.interval = Duration::from_secs(5);
    config.relay = true;
    config.modules = Some(Default::default());
    Agent::new(config).expect("agent")
}

/// Écoute qui accepte les connexions et ne répond jamais : l'équipement lent
/// ou muet d'un site distant. Rend l'adresse et le nombre de connexions tenues.
async fn tarpit() -> (std::net::SocketAddr, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("écoute");
    let address = listener.local_addr().expect("adresse");
    let held = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = held.clone();
    tokio::spawn(async move {
        let mut sockets = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            sockets.push(socket);
        }
    });
    (address, held)
}
