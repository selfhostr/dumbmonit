//! Configuration de l'agent : fichier YAML, surchargé par l'environnement.
//!
//! Les deux sources sont indispensables. Le fichier est ce que le script
//! d'installation écrit une fois pour toutes ; l'environnement est ce qui permet
//! de lancer l'agent dans un conteneur ou de tester une nouvelle URL sans toucher
//! au disque.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::collect::backup_repos::{self, BackupReposConfig, RepoConfig, Tool};
use crate::collect::databases::{self, BackupRule, DatabasesConfig, Engine};
use crate::collect::docker::DEFAULT_MAX_CONTAINERS;
use crate::collect::filter::NameFilter;
use crate::collect::ipsec::IpsecConfig;
use crate::collect::libvirt::{self, LibvirtConfig};
use crate::collect::perf_counters::{self, CustomCounter, PerfCountersConfig, PresetMode};
use crate::collect::plakar::{self, PlakarConfig};
use crate::collect::smart::{self, SmartConfig};
use crate::collect::speedtest::{self, SpeedtestConfig};
use crate::collect::system_health::SystemHealthConfig;
use crate::collect::wireguard::WireguardConfig;
use crate::collect::zfs::{self, ZfsConfig};
use crate::collect::{DEFAULT_INTERFACES_IGNORE, DEFAULT_MOUNTS_IGNORE, ProbeConfig};

mod merge;
mod parse;
#[cfg(test)]
mod tests;

use self::parse::*;

/// Un mégabit par seconde, en octets par seconde : c'est l'unité des options de
/// seuil du test de débit, plus lisible que des octets pour cette mesure-là.
const MBPS_IN_BYTES_PER_SECOND: f64 = 125_000.0;

/// Période d'échantillonnage par défaut. Trente secondes donnent des graphes
/// lisibles sans peser sur une machine modeste.
pub const DEFAULT_INTERVAL_SECS: u64 = 30;

/// En dessous, l'agent coûterait plus cher que ce qu'il mesure.
pub const MIN_INTERVAL_SECS: u64 = 5;

/// Borne du tampon de reprise, en échantillons.
///
/// Environ une heure de rattrapage pour une machine ordinaire, pour quelques
/// mégaoctets de mémoire : assez pour absorber le redémarrage d'un serveur sans
/// jamais menacer la machine surveillée.
pub const DEFAULT_MAX_BUFFERED_SAMPLES: usize = 20_000;

/// Borne par défaut du tampon disque, en mégaoctets (`buffer_max_mb`).
///
/// Les mesures y sont compressées : 256 Mo couvrent plusieurs jours de coupure
/// pour une machine ordinaire, et plusieurs heures pour un relais qui sonde
/// quelques dizaines d'équipements. Le tampon vide n'occupe rien.
pub const DEFAULT_BUFFER_MAX_MB: u64 = 256;

/// Chemin du socket Docker sur les systèmes de type Unix.
pub const DEFAULT_DOCKER_SOCKET: &str = "/var/run/docker.sock";

/// Période de l'inventaire des mises à jour en attente. Une heure : les dépôts
/// ne bougent pas plus vite, et chaque inventaire charge toutes leurs
/// métadonnées.
pub const DEFAULT_UPDATES_INTERVAL_SECS: u64 = 3600;

/// En dessous, l'agent passerait son temps à relancer `dnf`.
pub const MIN_UPDATES_INTERVAL_SECS: u64 = 300;

/// Modules optionnels qu'une liste `modules` peut retenir. La collecte système
/// de base (processeur, mémoire, disques, réseau) n'en fait pas partie : elle
/// est toujours là, c'est elle qui dit que la machine répond.
pub const MODULES: &[&str] = &[
    "docker",
    "plakar",
    "system_health",
    "sensors",
    "smart",
    "zfs",
    "libvirt",
    "wireguard",
    "ipsec",
    "databases",
    "perf_counters",
];

/// Ce que le fichier YAML peut contenir.
///
/// Tous les champs sont optionnels : un fichier ne portant que l'URL et le jeton
/// est parfaitement valide, et c'est exactement ce qu'écrit l'installateur.
/// Les champs inconnus sont ignorés plutôt que refusés, pour qu'un fichier écrit
/// par une version plus récente ne bloque pas le démarrage.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    server_url: Option<String>,
    server_urls: Option<ServerListFile>,
    token: Option<String>,
    tls_url: Option<String>,
    ca_pin: Option<String>,
    server_pin: Option<String>,
    transport: Option<String>,
    interval_secs: Option<u64>,
    hostname: Option<String>,
    services: Option<Vec<String>>,
    tags: Option<BTreeMap<String, String>>,
    modules: Option<Patterns>,
    docker: Option<bool>,
    docker_socket: Option<String>,
    docker_update_check: Option<bool>,
    docker_max_containers: Option<usize>,
    commands: Option<bool>,
    actions: Option<bool>,
    actions_dir: Option<String>,
    action_services: Option<Vec<String>>,
    scripts: Option<bool>,
    scripts_dirs: Option<Vec<String>>,
    scripts_user: Option<String>,
    scripts_max_concurrent: Option<usize>,
    relay: Option<bool>,
    site: Option<String>,
    gateway_listen: Option<String>,
    gateway_max_connections: Option<usize>,
    gateway_rate_kib: Option<u32>,
    interfaces_ignore: Option<Patterns>,
    interfaces_only: Option<Patterns>,
    mounts_ignore: Option<Patterns>,
    cpu_per_core: Option<bool>,
    plakar: Option<bool>,
    plakar_bin: Option<String>,
    plakar_klosets: Option<Vec<String>>,
    plakar_home: Option<String>,
    plakar_interval_secs: Option<u64>,
    sensors: Option<bool>,
    smart: Option<bool>,
    smart_bin: Option<String>,
    smart_interval_secs: Option<u64>,
    zfs: Option<bool>,
    zfs_bin: Option<String>,
    zfs_interval_secs: Option<u64>,
    libvirt: Option<bool>,
    libvirt_bin: Option<String>,
    libvirt_uri: Option<String>,
    libvirt_interval_secs: Option<u64>,
    wireguard: Option<bool>,
    wireguard_bin: Option<String>,
    wireguard_peer_names: Option<BTreeMap<String, String>>,
    ipsec: Option<bool>,
    ipsec_bin: Option<String>,
    inventory: Option<bool>,
    self_update: Option<bool>,
    speedtest: Option<bool>,
    speedtest_download_url: Option<String>,
    speedtest_upload_url: Option<String>,
    speedtest_download_bytes_cap: Option<u64>,
    speedtest_upload_bytes: Option<u64>,
    speedtest_interval_secs: Option<u64>,
    speedtest_min_download_mbps: Option<f64>,
    speedtest_min_upload_mbps: Option<f64>,
    speedtest_max_latency_ms: Option<f64>,
    restic_bin: Option<String>,
    borg_bin: Option<String>,
    restic_repos: Option<Vec<RepoFile>>,
    borg_repos: Option<Vec<RepoFile>>,
    backup_repos_interval_secs: Option<u64>,
    perf_counters: Option<Vec<PerfCounterFile>>,
    mdaemon: Option<FlagOrAuto>,
    hyperv: Option<FlagOrAuto>,
    max_buffered_samples: Option<usize>,
    buffer_max_mb: Option<u64>,
    log_level: Option<String>,
    proxy: Option<String>,
    no_proxy: Option<Patterns>,
    proxy_user: Option<String>,
    proxy_password: Option<String>,
    system_health: Option<SystemHealthFile>,
    databases: Option<DatabasesFile>,
}

/// Liste de motifs, écrite en YAML soit comme une liste, soit comme une seule
/// chaîne dont les éléments sont séparés par des virgules — la même forme que
/// la variable d'environnement, pour qu'un exemple se copie de l'une à l'autre.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum Patterns {
    One(String),
    Many(Vec<String>),
}

impl Patterns {
    fn into_items(self) -> Vec<String> {
        match self {
            Self::One(text) => split_list(&text),
            Self::Many(items) => items
                .iter()
                .map(|item| item.trim())
                .filter(|i| !i.is_empty())
                .map(String::from)
                .collect(),
        }
    }
}

/// Un compteur de performance Windows : son chemin seul, ou son chemin et le
/// nom sous lequel le publier.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum PerfCounterFile {
    Path(String),
    Named { path: String, name: Option<String> },
}

/// Adresses de serveur de `server_urls`, dans l'ordre : une liste, ou une
/// seule chaîne aux éléments séparés par des virgules (la forme de
/// `DUMBMONIT_AGENT_URLS`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum ServerListFile {
    One(String),
    Many(Vec<ServerEntryFile>),
}

/// Une adresse de serveur : son URL seule, ou son URL et celle de son canal
/// chiffré quand il n'est pas sur le même hôte au même port que la primaire.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum ServerEntryFile {
    Url(String),
    Full { url: String, tls_url: Option<String> },
}

impl ServerListFile {
    fn into_endpoints(self) -> Vec<ServerEndpoint> {
        match self {
            Self::One(text) => split_list(&text)
                .into_iter()
                .map(|url| ServerEndpoint { url, tls_url: None })
                .collect(),
            Self::Many(entries) => entries
                .into_iter()
                .map(|entry| match entry {
                    ServerEntryFile::Url(url) => ServerEndpoint { url, tls_url: None },
                    ServerEntryFile::Full { url, tls_url } => ServerEndpoint { url, tls_url },
                })
                .collect(),
        }
    }
}

/// Un serveur DumbMonit que l'agent sait joindre : la racine de son
/// interface, et l'adresse de son canal chiffré si elle est donnée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEndpoint {
    /// Racine du serveur (`https://hôte:8080`), sans barre oblique finale.
    pub url: String,
    /// Canal chiffré (`https://hôte:8443`). `None` : même hôte que `url`, au
    /// port du canal de la primaire.
    pub tls_url: Option<String>,
}

/// Racine d'une URL saisie : sans espaces ni barre oblique finale.
fn clean_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

/// `true`, `false` ou `auto` : YAML lit les deux premiers comme des booléens,
/// le troisième comme du texte.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum FlagOrAuto {
    Flag(bool),
    Text(String),
}

/// Un dépôt restic ou Borg déclaré dans le fichier. Le mot de passe n'y figure
/// qu'indirectement — un fichier, une commande, ou une variable de `env` — et ne
/// quitte jamais la machine.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RepoFile {
    name: Option<String>,
    repository: Option<String>,
    password_file: Option<String>,
    password_command: Option<String>,
    env: BTreeMap<String, String>,
    args: Vec<String>,
    host: Option<String>,
    glob_archives: Option<String>,
}

/// Section `databases` du fichier : bases de données détectées et dossiers de
/// leurs sauvegardes.
///
/// ```text
/// databases:
///   backups:
///     sqlserver:
///       paths: ['D:\Backups\SQL\*.bak']
///       max_age_hours: 26
/// ```
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DatabasesFile {
    enabled: Option<bool>,
    #[serde(alias = "interval_seconds")]
    interval_secs: Option<u64>,
    backups: BTreeMap<String, DatabaseBackupFile>,
}

/// Les sauvegardes d'un moteur : des chemins (jokers admis) et un âge maximal.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DatabaseBackupFile {
    paths: Vec<String>,
    max_age_hours: Option<u64>,
}

/// Section `system_health` du fichier : santé du système d'exploitation.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SystemHealthFile {
    enabled: Option<bool>,
    check_updates: Option<bool>,
    #[serde(alias = "updates_interval_seconds")]
    updates_interval_secs: Option<u64>,
    check_reboot: Option<bool>,
    check_failed_units: Option<bool>,
}

/// Configuration effective, une fois le fichier et l'environnement fusionnés.
#[derive(Clone)]
pub struct Config {
    /// Racine du serveur DumbMonit, sans le chemin de la route.
    pub server_url: String,
    /// Jeton d'enregistrement. Ne doit jamais apparaître dans les journaux.
    pub token: String,
    /// Canal chiffré des agents (`https://hôte:8443`), tel que la commande
    /// d'installation l'écrit. `None` : même hôte que `server_url`, port annoncé
    /// par le serveur.
    pub tls_url: Option<String>,
    /// Empreinte SHA-256 de l'autorité du serveur. Posée, l'agent ne parle
    /// qu'au canal chiffré et ne reconnaît que ce serveur.
    pub ca_pin: Option<String>,
    /// Clé publique épinglée (`sha256//…`) du certificat autosigné du port de
    /// l'interface, que la commande d'installation a vérifiée : l'ancien
    /// canal ne reconnaît alors que ce serveur.
    pub server_pin: Option<String>,
    /// `server_url` est en `http://` hors boucle locale (agent installé avant
    /// le passage au tout-HTTPS) : seul le canal chiffré déjà enrôlé sert,
    /// jamais l'ancien canal en clair.
    pub plain_server_url: bool,
    /// Serveurs de secours, dans l'ordre (`server_urls` sans la primaire) :
    /// l'agent y passe quand la primaire ne répond plus, et revient à elle dès
    /// qu'elle répond (voir `failover.rs`). Vide : un seul serveur, rien ne
    /// change.
    pub fallback_servers: Vec<ServerEndpoint>,
    /// Transport du canal chiffré (`transport`, `DUMBMONIT_AGENT_TRANSPORT`) :
    /// `auto` (défaut : le port des agents, puis la WebSocket de l'interface
    /// s'il est injoignable), `direct` ou `websocket` (voir `ws.rs`).
    pub transport: crate::ws::TransportMode,
    pub interval: Duration,
    /// Nom d'hôte annoncé au serveur. `None` : celui que le système déclare.
    pub hostname: Option<String>,
    /// Unités systemd ou services Windows dont l'état est remonté.
    pub services: Vec<String>,
    pub tags: BTreeMap<String, String>,
    /// Modules retenus par la liste `modules`. `None` : pas de liste, chaque
    /// module se détecte seul.
    pub modules: Option<BTreeSet<String>>,
    pub docker: bool,
    pub docker_socket: PathBuf,
    /// Comparer les images des conteneurs à leur dépôt, une fois par heure.
    pub docker_update_check: bool,
    /// Au-delà, les conteneurs sont comptés mais plus détaillés.
    pub docker_max_containers: usize,
    /// Accepter les actions envoyées par le serveur (redémarrage, mise à jour
    /// de conteneur). Faux : l'agent ne fait que mesurer.
    pub commands: bool,
    /// Actions de remédiation (services, scripts locaux). Désactivées par
    /// défaut, et bornées par ce que la machine déclare : voir `remediation.rs`.
    pub actions: crate::remediation::ActionsConfig,
    /// Contrôles personnalisés (scripts, greffons Nagios). Désactivés par
    /// défaut, et bornés aux dossiers que la machine autorise : voir
    /// `scripts/mod.rs`.
    pub scripts: crate::scripts::ScriptsConfig,
    /// Relayer les interrogations que le serveur délègue à cet agent : il
    /// exécute alors lui-même les collecteurs (SNMP, Proxmox, HTTP…) contre les
    /// équipements de son propre réseau. Désactivé par défaut : un agent
    /// ordinaire n'a pas à sortir de sa machine.
    pub relay: bool,
    /// Site où l'agent est posé, tel qu'affiché dans l'interface.
    pub site: Option<String>,
    /// Passerelle du relais pour les agents de son site (voir `gateway.rs`).
    pub gateway: crate::gateway::GatewayConfig,
    /// Périmètre de la collecte système : interfaces, montages, détail par cœur.
    pub probe: ProbeConfig,
    /// Santé du système : mises à jour, redémarrage, unités en échec, SELinux.
    pub system_health: SystemHealthConfig,
    /// Sauvegardes Plakar.
    pub plakar: PlakarConfig,
    /// Lire les sondes de température et les ventilateurs de la machine.
    pub sensors: bool,
    /// Santé des disques, par `smartctl`.
    pub smart: SmartConfig,
    /// Pools ZFS, par `zpool`.
    pub zfs: ZfsConfig,
    /// Domaines KVM et pools de stockage libvirt, par `virsh`.
    pub libvirt: LibvirtConfig,
    /// Tunnels WireGuard, par `wg show all dump`.
    pub wireguard: WireguardConfig,
    /// Tunnels IPsec (strongSwan), par `swanctl`.
    pub ipsec: IpsecConfig,
    /// Test de débit Internet. Désactivé par défaut.
    pub speedtest: SpeedtestConfig,
    /// Inventaire logiciel quotidien (paquets installés), joint aux lots. Actif
    /// par défaut ; indépendant de la liste `modules`.
    pub inventory: bool,
    /// Mise à jour automatique de l'agent par le serveur. Active par défaut ;
    /// `false` la refuse sur cette machine, quels que soient les réglages du
    /// serveur.
    pub self_update: bool,
    /// Dépôts restic et Borg déclarés.
    pub backup_repos: BackupReposConfig,
    /// Bases de données détectées et fraîcheur de leurs sauvegardes.
    pub databases: DatabasesConfig,
    /// Compteurs de performance Windows : liste libre, jeux MDaemon et Hyper-V.
    pub perf_counters: PerfCountersConfig,
    pub max_buffered_samples: usize,
    /// Borne du tampon disque (mesures de la machine et résultats du relais),
    /// en mégaoctets ; zéro le désactive : l'agent ne garde alors que la
    /// mémoire, comme avant.
    pub buffer_max_mb: u64,
    /// Fichier où l'agent range le secret de liaison que le serveur lui
    /// attribue. À côté de la configuration par défaut, pour qu'un déplacement
    /// de l'une emmène l'autre.
    pub secret_path: PathBuf,
    pub log_level: tracing::Level,
    /// Mandataire sortant vers le serveur (voir `proxy.rs`).
    pub proxy: crate::proxy::ProxyConfig,
}

/// Le jeton n'est jamais affiché, pas même tronqué.
///
/// C'est la seule protection qui tienne : un `Debug` dérivé finirait tôt ou tard
/// dans une trace d'erreur, et un jeton d'enregistrement ouvre l'ingestion.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("server_url", &self.server_url)
            .field("token", &"<redacted>")
            .field("tls_url", &self.tls_url)
            .field("ca_pin", &self.ca_pin)
            .field("server_pin", &self.server_pin)
            .field("plain_server_url", &self.plain_server_url)
            .field("fallback_servers", &self.fallback_servers)
            .field("transport", &self.transport)
            .field("interval", &self.interval)
            .field("hostname", &self.hostname)
            .field("services", &self.services)
            .field("tags", &self.tags)
            .field("modules", &self.modules)
            .field("docker", &self.docker)
            .field("docker_socket", &self.docker_socket)
            .field("docker_update_check", &self.docker_update_check)
            .field("docker_max_containers", &self.docker_max_containers)
            .field("commands", &self.commands)
            .field("actions", &self.actions)
            .field("scripts", &self.scripts)
            .field("relay", &self.relay)
            .field("site", &self.site)
            .field("gateway", &self.gateway)
            .field("probe", &self.probe)
            .field("max_buffered_samples", &self.max_buffered_samples)
            .field("buffer_max_mb", &self.buffer_max_mb)
            .field("log_level", &self.log_level)
            .field("proxy", &self.proxy)
            .field("system_health", &self.system_health)
            .field("plakar", &self.plakar)
            .field("sensors", &self.sensors)
            .field("smart", &self.smart)
            .field("zfs", &self.zfs)
            .field("libvirt", &self.libvirt)
            .field("wireguard", &self.wireguard)
            .field("ipsec", &self.ipsec)
            .field("speedtest", &self.speedtest)
            .field("inventory", &self.inventory)
            .field("self_update", &self.self_update)
            .field("backup_repos", &self.backup_repos)
            .field("databases", &self.databases)
            .field("perf_counters", &self.perf_counters)
            .finish()
    }
}

impl Config {
    /// Emplacement du secret de liaison lorsque rien ne le précise.
    ///
    /// `DUMBMONIT_AGENT_SECRET_FILE` prime — c'est ce qu'on monte en volume pour
    /// un agent en conteneur, dont le `/etc` disparaît à chaque recréation.
    fn resolve_secret_path(env: &EnvSource, config_path: Option<&Path>) -> PathBuf {
        if let Some(explicit) = env.get("DUMBMONIT_AGENT_SECRET_FILE") {
            let explicit = explicit.trim();
            if !explicit.is_empty() {
                return PathBuf::from(explicit);
            }
        }
        match config_path {
            Some(path) => crate::binding::beside(path),
            None => crate::binding::beside(&Self::default_path()),
        }
    }

    /// Emplacement du fichier de configuration lorsque rien n'est précisé.
    ///
    /// Le même que celui où `install.sh` et `install.ps1` l'écrivent, pour que
    /// `dumbmonit-agent --dry-run` lancé à la main lise la configuration du
    /// service. Sous macOS et FreeBSD, `/etc/dumbmonit` reste reconnu tant que
    /// le premier emplacement n'existe pas : c'était l'emplacement par défaut
    /// avant que l'agent ne suive le script.
    pub fn default_path() -> PathBuf {
        let (current, fallback) = Self::default_paths();
        match fallback {
            Some(fallback) if !current.exists() && fallback.exists() => fallback,
            _ => current,
        }
    }

    fn default_paths() -> (PathBuf, Option<PathBuf>) {
        #[cfg(windows)]
        {
            let root =
                std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
            let root = PathBuf::from(root);
            (root.join("DumbMonit").join("agent.yaml"), None)
        }
        // macOS réserve `/etc` au système, et les ports de FreeBSD rangent tout
        // ce qui n'est pas la base sous `/usr/local` : c'est là qu'écrit install.sh.
        #[cfg(any(target_os = "macos", target_os = "freebsd"))]
        {
            (
                PathBuf::from("/usr/local/etc/dumbmonit/agent.yaml"),
                Some(PathBuf::from("/etc/dumbmonit/agent.yaml")),
            )
        }
        #[cfg(not(any(windows, target_os = "macos", target_os = "freebsd")))]
        {
            (PathBuf::from("/etc/dumbmonit/agent.yaml"), None)
        }
    }

    /// Charge la configuration : fichier s'il existe, puis surcharges d'environnement.
    ///
    /// L'absence de fichier n'est pas une erreur : elle correspond au cas
    /// « tout est dans l'environnement », typique d'un déploiement en conteneur.
    pub fn load(path: &Path) -> Result<Self> {
        let file = match std::fs::read_to_string(path) {
            Ok(text) => dumbmonit_collectors::yaml::from_str::<FileConfig>(&text)
                .with_context(|| format!("unreadable configuration file: {}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => FileConfig::default(),
            Err(error) => {
                return Err(
                    anyhow::Error::from(error).context(format!("reading {}", path.display()))
                );
            }
        };
        Self::merge_at(file, EnvSource::process(), Some(path))
    }

    /// Fusion sans fichier de configuration sur le disque : les tests s'en
    /// servent pour exercer les surcharges d'environnement à l'unité.
    #[cfg(test)]
    fn merge(file: FileConfig, env: EnvSource) -> Result<Self> {
        Self::merge_at(file, env, None)
    }
}

/// Source des surcharges. Indirection volontaire : elle rend la fusion testable
/// sans toucher aux variables d'environnement du processus de test, qui sont
/// globales et donc hostiles à l'exécution parallèle des tests.
struct EnvSource {
    vars: BTreeMap<String, String>,
}

impl EnvSource {
    fn process() -> Self {
        Self::from_map(std::env::vars().collect())
    }

    fn from_map(vars: BTreeMap<String, String>) -> Self {
        Self { vars }
    }

    /// Lit une variable `DUMBMONIT_AGENT_*` ; une valeur vide est traitée comme absente.
    fn get(&self, key: &str) -> Option<String> {
        let value = self.vars.get(key)?.trim();
        (!value.is_empty()).then(|| value.to_string())
    }
}
