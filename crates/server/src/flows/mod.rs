//! Flux réseau (bêta, désactivé par défaut) : « qui parle à qui » par
//! équipement, à partir de NetFlow v5/v9, IPFIX et sFlow v5.
//!
//! Un récepteur UDP par protocole, dans le serveur, ouvert depuis Réglages →
//! Système. Seuls les datagrammes dont l'adresse source est celle d'une cible
//! surveillée sont lus (table des émetteurs des journaux, [`attach`]) ; les
//! autres sont comptés et écartés. Les échanges sont agrégés par minute
//! ([`aggregate`]) puis écrits dans VictoriaMetrics comme les autres métriques
//! (`dumbmonit_flow_bytes_total`, `dumbmonit_flow_packets_total`), donc gardés
//! aussi longtemps qu'elles.
//!
//! [`attach`]: crate::logs::attach

pub mod aggregate;
pub mod cursor;
pub mod netflow;
pub mod sflow;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::net::UdpSocket;
use tokio::sync::watch;

use crate::state::AppState;
use crate::tsdb::SampleSink;

/// Clé du réglage dans la table `settings`.
pub const SETTINGS_KEY: &str = "flows.settings";
pub const DEFAULT_NETFLOW_PORT: u16 = 2055;
pub const DEFAULT_SFLOW_PORT: u16 = 6343;
const MAX_DATAGRAM: usize = 65_535;
const FLUSH_EVERY: Duration = Duration::from_secs(15);
/// Exportateurs et sources inconnues gardés pour l'écran des réglages.
const MAX_EXPORTERS: usize = 1000;
const MAX_UNKNOWN: usize = 5;

/// Un échange : deux adresses, le protocole IP, le port du service, et le
/// volume (déjà multiplié par le taux d'échantillonnage).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowRecord {
    pub src: IpAddr,
    pub dst: IpAddr,
    pub proto: u8,
    /// Port du service : le plus bas des deux (le port éphémère du client est
    /// presque toujours le plus haut) ; zéro hors TCP, UDP et SCTP.
    pub port: u16,
    pub bytes: u64,
    pub packets: u64,
}

impl FlowRecord {
    pub fn new(
        src: IpAddr,
        dst: IpAddr,
        proto: u8,
        src_port: u16,
        dst_port: u16,
        bytes: u64,
        packets: u64,
    ) -> Self {
        Self { src, dst, proto, port: service_port(proto, src_port, dst_port), bytes, packets }
    }
}

pub fn service_port(proto: u8, src_port: u16, dst_port: u16) -> u16 {
    if !matches!(proto, 6 | 17 | 132) {
        return 0;
    }
    match (src_port, dst_port) {
        (0, port) | (port, 0) => port,
        (a, b) => a.min(b),
    }
}

// --------------------------------------------------------------------------
// Réglage
// --------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// NetFlow v5/v9 et IPFIX.
    pub netflow_port: u16,
    pub sflow_port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            netflow_port: DEFAULT_NETFLOW_PORT,
            sflow_port: DEFAULT_SFLOW_PORT,
        }
    }
}

impl Settings {
    /// Message pour l'interface, en anglais comme les autres erreurs d'API.
    pub fn validate(&self) -> Result<(), String> {
        if self.netflow_port == 0 || self.sflow_port == 0 {
            return Err("Ports must be between 1 and 65535.".into());
        }
        if self.netflow_port == self.sflow_port {
            return Err("NetFlow/IPFIX and sFlow need two different ports.".into());
        }
        Ok(())
    }
}

pub async fn load(pool: &SqlitePool) -> Result<Settings> {
    Ok(crate::auth::settings::get(pool, SETTINGS_KEY).await?.unwrap_or_default())
}

pub async fn save(pool: &SqlitePool, settings: &Settings) -> Result<()> {
    crate::auth::settings::set(pool, SETTINGS_KEY, settings).await
}

// --------------------------------------------------------------------------
// État en mémoire
// --------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Netflow,
    Sflow,
}

#[derive(Debug, Default)]
pub struct Counters {
    pub datagrams: AtomicU64,
    pub flows: AtomicU64,
    pub unknown_exporter: AtomicU64,
    pub malformed: AtomicU64,
    pub missing_template: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CountersView {
    pub datagrams: u64,
    pub flows: u64,
    pub dropped_unknown: u64,
    pub dropped_malformed: u64,
    pub missing_template: u64,
    pub series_capped: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListenerStatus {
    pub protocol: Protocol,
    pub port: u16,
    pub listening: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exporter {
    pub target_id: i64,
    pub target_name: String,
    pub address: String,
    pub protocol: Protocol,
    pub datagrams: u64,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnknownSource {
    pub address: String,
    pub last_seen: String,
}

#[derive(Default)]
pub struct Hub {
    pub counters: Counters,
    listeners: Mutex<Vec<ListenerStatus>>,
    exporters: Mutex<HashMap<(i64, Protocol), Exporter>>,
    unknown: Mutex<VecDeque<UnknownSource>>,
    aggregator: Mutex<aggregate::Aggregator>,
    templates: Mutex<netflow::Templates>,
    control: OnceLock<watch::Sender<Settings>>,
}

static HUB: OnceLock<Hub> = OnceLock::new();

pub fn hub() -> &'static Hub {
    HUB.get_or_init(Hub::default)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Secondes depuis le premier appel : horloge monotone des gabarits.
fn mono_secs() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs()
}

impl Hub {
    pub fn counters(&self) -> CountersView {
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        CountersView {
            datagrams: read(&self.counters.datagrams),
            flows: read(&self.counters.flows),
            dropped_unknown: read(&self.counters.unknown_exporter),
            dropped_malformed: read(&self.counters.malformed),
            missing_template: read(&self.counters.missing_template),
            series_capped: lock(&self.aggregator).capped,
        }
    }

    pub fn listeners(&self) -> Vec<ListenerStatus> {
        lock(&self.listeners).clone()
    }

    /// Les exportateurs reconnus, le plus récent d'abord.
    pub fn exporters(&self) -> Vec<Exporter> {
        let mut list: Vec<Exporter> = lock(&self.exporters).values().cloned().collect();
        list.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then(a.target_id.cmp(&b.target_id)));
        list
    }

    pub fn unknown_sources(&self) -> Vec<UnknownSource> {
        lock(&self.unknown).iter().cloned().collect()
    }

    /// Applique un nouveau réglage au récepteur, s'il tourne (jamais dans les
    /// tests : seul `main` le lance).
    pub fn apply(&self, settings: &Settings) {
        if let Some(control) = self.control.get() {
            control.send_replace(settings.clone());
        }
    }

    fn set_listener(&self, status: ListenerStatus) {
        let mut listeners = lock(&self.listeners);
        listeners.retain(|held| held.protocol != status.protocol);
        listeners.push(status);
        listeners.sort_by_key(|held| held.protocol as u8);
    }

    /// Traite un datagramme reçu de `from`. `lookup` donne la cible d'une
    /// adresse (la table des émetteurs en service, une table fixe en test).
    pub fn receive(
        &self,
        protocol: Protocol,
        data: &[u8],
        from: IpAddr,
        now: DateTime<Utc>,
        lookup: impl Fn(IpAddr) -> Option<(i64, String)>,
    ) {
        let from = from.to_canonical();
        self.counters.datagrams.fetch_add(1, Ordering::Relaxed);
        let stamp = crate::db::timestamp(now);
        let Some((target, name)) = lookup(from) else {
            self.counters.unknown_exporter.fetch_add(1, Ordering::Relaxed);
            let mut unknown = lock(&self.unknown);
            let address = from.to_string();
            unknown.retain(|held| held.address != address);
            unknown.push_front(UnknownSource { address, last_seen: stamp });
            unknown.truncate(MAX_UNKNOWN);
            return;
        };
        let records = match protocol {
            Protocol::Netflow => {
                let mut templates = lock(&self.templates);
                netflow::decode(data, from, mono_secs(), &mut templates).map(|decoded| {
                    let missing = decoded.missing_template;
                    self.counters.missing_template.fetch_add(missing, Ordering::Relaxed);
                    decoded.records
                })
            }
            Protocol::Sflow => sflow::decode(data).map_err(|_| netflow::DecodeError::Malformed),
        };
        let Ok(records) = records else {
            self.counters.malformed.fetch_add(1, Ordering::Relaxed);
            return;
        };
        self.counters.flows.fetch_add(records.len() as u64, Ordering::Relaxed);
        {
            let mut exporters = lock(&self.exporters);
            if exporters.len() < MAX_EXPORTERS || exporters.contains_key(&(target, protocol)) {
                let entry = exporters.entry((target, protocol)).or_insert_with(|| Exporter {
                    target_id: target,
                    target_name: name.clone(),
                    address: from.to_string(),
                    protocol,
                    datagrams: 0,
                    last_seen: stamp.clone(),
                });
                entry.target_name = name;
                entry.address = from.to_string();
                entry.datagrams += 1;
                entry.last_seen = stamp;
            }
        }
        let minute = now.timestamp().div_euclid(60);
        let mut aggregator = lock(&self.aggregator);
        for record in &records {
            aggregator.add(target, minute, record);
        }
    }

    /// Les minutes closes à `now`, prêtes à écrire.
    pub fn drain(&self, now: DateTime<Utc>) -> Vec<dumbmonit_proto::Sample> {
        lock(&self.templates).expire(mono_secs());
        lock(&self.aggregator).drain(now.timestamp().div_euclid(60))
    }
}

// --------------------------------------------------------------------------
// Récepteurs
// --------------------------------------------------------------------------

/// Lance le superviseur des récepteurs : il (re)lit le réglage, ouvre ou
/// ferme les ports, et écrit les minutes closes.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let settings = match load(&state.pool).await {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "flow settings unreadable, off");
                Settings::default()
            }
        };
        let (control, mut changes) = watch::channel(settings);
        if hub().control.set(control).is_err() {
            return;
        }
        tokio::spawn(flusher(state.sink.clone()));
        loop {
            let current = changes.borrow_and_update().clone();
            let tasks = start(&current);
            if changes.changed().await.is_err() {
                break;
            }
            for task in tasks {
                task.abort();
            }
            // Le temps que les sockets se ferment avant d'ouvrir les nouveaux.
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
}

fn start(settings: &Settings) -> Vec<tokio::task::JoinHandle<()>> {
    let ports =
        [(Protocol::Netflow, settings.netflow_port), (Protocol::Sflow, settings.sflow_port)];
    if !settings.enabled {
        for (protocol, port) in ports {
            hub().set_listener(ListenerStatus { protocol, port, listening: false, error: None });
        }
        return Vec::new();
    }
    ports.into_iter().map(|(protocol, port)| tokio::spawn(listen(protocol, port))).collect()
}

async fn listen(protocol: Protocol, port: u16) {
    let address = SocketAddr::new(crate::logs::central::env_bind("DUMBMONIT_FLOWS_BIND"), port);
    let mut attempts = 0;
    let socket = loop {
        match UdpSocket::bind(address).await {
            Ok(socket) => break socket,
            // L'ancien socket du même port peut mettre un instant à se fermer.
            Err(_) if attempts < 5 => {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(error) => {
                tracing::warn!(%address, %error, "flow port unavailable");
                let error = Some(format!("UDP {address}: {error}"));
                hub().set_listener(ListenerStatus { protocol, port, listening: false, error });
                return;
            }
        }
    };
    tracing::info!(%address, ?protocol, "flow receiver listening");
    hub().set_listener(ListenerStatus { protocol, port, listening: true, error: None });
    let mut buffer = vec![0u8; MAX_DATAGRAM];
    loop {
        let (length, from) = match socket.recv_from(&mut buffer).await {
            Ok(received) => received,
            Err(error) => {
                tracing::debug!(%error, "flow receive failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        hub().receive(protocol, &buffer[..length], from.ip(), Utc::now(), |ip| {
            crate::logs::attach::resolver().lookup(ip, None)
        });
    }
}

async fn flusher(sink: SampleSink) {
    let mut every = tokio::time::interval(FLUSH_EVERY);
    loop {
        every.tick().await;
        let samples = hub().drain(Utc::now());
        sink.send(samples).await;
    }
}
