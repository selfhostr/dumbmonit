//! Boucle de vie de l'agent : collecter, envoyer, recommencer.

use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::{Context, Result};
use dumbmonit_proto::{
    AgentIdentity, AgentUpdateOffer, MAX_BATCH_SAMPLES, MetricKind, PushBatch, RelaySetting,
    Sample, SoftwareInventory,
};
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::backoff::Backoff;
use crate::binding::Binding;
use crate::buffer::PendingBuffer;
use crate::client::{PushClient, PushError};
use crate::collect::backup_repos::BackupReposProbe;
use crate::collect::databases::DatabasesProbe;
use crate::collect::docker::DockerProbe;
use crate::collect::dsm::DsmProbe;
use crate::collect::eventlog::{self, PlatformProbe as EventLogProbe};
use crate::collect::inventory::{self, PendingInventory};
use crate::collect::ipsec::IpsecProbe;
use crate::collect::libvirt::LibvirtProbe;
use crate::collect::machine::MachineProbe;
use crate::collect::mail_server::PlatformProbe as MailServerProbe;
use crate::collect::perf_counters::PlatformProbe as PerfCountersProbe;
use crate::collect::plakar::PlakarProbe;
use crate::collect::registry::UpdateChecker;
use crate::collect::sensors::SensorsProbe;
use crate::collect::smart::SmartProbe;
use crate::collect::speedtest::SpeedtestProbe;
use crate::collect::system_health::SystemHealthProbe;
use crate::collect::wireguard::WireguardProbe;
use crate::collect::zfs::ZfsProbe;
use crate::collect::{SystemProbe, agent_samples, services};
use crate::commands::CommandRunner;
use crate::config::{Config, MIN_INTERVAL_SECS};
use crate::failover::{self, Failover};
use crate::logs::LogRunner;
use crate::relay::{Offline, RelayRunner};
use crate::relay_offline::{Backlog, PlanStore};
use crate::selfupdate::{AfterStop, SelfUpdater};
use crate::sentinel::SentinelRunner;
use crate::shutdown::Shutdown;
use crate::spool::Spool;
use crate::tls::TlsState;

mod channel;
mod flush;
mod relay_gateway;
mod startup;

use self::startup::{initial_tls, open_sample_spool, plain_url_channel};
#[cfg(test)]
mod tests;

/// Délai avant une nouvelle tentative d'enrôlement ou de renouvellement sur le
/// canal chiffré, après un échec.
const TLS_RETRY: Duration = Duration::from_secs(60);

/// Échecs de connexion d'affilée au canal chiffré avant d'essayer l'autre
/// transport (`transport: auto`) : port des agents ou WebSocket de l'interface.
const TRANSPORT_FALLBACK_AFTER: u32 = 2;

/// Période de l'essai de la passerelle par le relais, une fois ouverte et
/// essayée : un pare-feu, une publication de port ou une adresse peuvent
/// changer sans que rien ne le dise à l'agent.
const GATEWAY_RECHECK: Duration = Duration::from_secs(3600);

/// Période de l'essai tant qu'il échoue.
const GATEWAY_RETRY: Duration = Duration::from_secs(60);

/// Essai de la passerelle partagé avec la tâche qui le mène.
type GatewayCheckSlot = std::sync::Arc<std::sync::Mutex<Option<dumbmonit_proto::GatewayCheck>>>;

/// Première temporisation après un échec d'envoi.
const BACKOFF_BASE: Duration = Duration::from_secs(2);

/// Plafond de la temporisation : trente secondes, décalage aléatoire compris.
///
/// Il était de cinq minutes. Le serveur déclare une machine muette après trois
/// périodes, au moins 90 s (`staleness_deadline`) : un plafond plus long que ce
/// délai transformait chaque coupure en panne apparente rallongée de plusieurs
/// minutes après le retour du serveur — 15 tentatives sur 53 min observées pour
/// une coupure réseau. Un agent toutes les 30 s ne pèse rien sur un serveur qui
/// redémarre ; le décalage aléatoire étale le parc.
pub(crate) const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Mesures gardées sur disque à l'arrêt quand le serveur ne répond pas, au plus.
/// Quelques mégaoctets : de quoi couvrir un redémarrage pendant une coupure,
/// pas de quoi remplir un disque.
const MAX_PERSISTED_SAMPLES: usize = 20_000;

/// Pause entre deux lots de rattrapage tirés du tampon disque : le retard part
/// vite, sans prendre toute la liaison d'un petit site.
const CATCHUP_PAUSE: Duration = Duration::from_millis(250);

/// Délai maximal d'un envoi. Généreux, car un lot de rattrapage est volumineux et
/// part parfois sur une liaison lente.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Temps laissé au dernier envoi lors de l'arrêt. Court : systemd n'attend pas
/// indéfiniment, et être tué en essayant d'être poli ne rend service à personne.
const SHUTDOWN_FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// Temps laissé à une commande en cours lors de l'arrêt. Une mise à jour coupée
/// en plein milieu laisserait un conteneur renommé : on attend, mais moins que
/// les 90 s au bout desquelles systemd tue le processus.
const SHUTDOWN_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// En `--once`, la commande éventuelle est menée à son terme : personne ne
/// viendra la finir après nous.
const ONCE_COMMAND_TIMEOUT: Duration = Duration::from_secs(900);

/// Délai d'une requête SNMP unitaire en mode relais. Le délai global de chaque
/// sonde vient du serveur avec la sonde elle-même.
const RELAY_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Agent {
    config: Config,
    identity: AgentIdentity,
    probe: SystemProbe,
    health: SystemHealthProbe,
    docker: DockerProbe,
    updates: UpdateChecker,
    plakar: PlakarProbe,
    /// Dépôts restic et Borg déclarés dans la configuration.
    backup_repos: BackupReposProbe,
    /// Bases de données et fraîcheur de leurs sauvegardes : muet sans base.
    databases: DatabasesProbe,
    /// Tunnels WireGuard : muet sans `wg` ni interface.
    wireguard: WireguardProbe,
    /// Tunnels IPsec : muet sans `swanctl` ni connexion.
    ipsec: IpsecProbe,
    /// Test de débit Internet : muet tant qu'il n'est pas activé.
    speedtest: SpeedtestProbe,
    /// Matériel : sondes de température, santé des disques, pools ZFS. Chacun
    /// se tait complètement là où la machine n'a rien à en dire.
    sensors: SensorsProbe,
    /// Top processus, PSI, inodes, horloge, OOM kills : muet hors Linux.
    machine: MachineProbe,
    smart: SmartProbe,
    zfs: ZfsProbe,
    /// Domaines KVM et pools libvirt. Muet sans `virsh` ni domaine.
    libvirt: LibvirtProbe,
    /// Synology DSM : grappes, volumes, disques, paquets. Muet hors DSM.
    dsm: DsmProbe,
    /// Dernier inventaire logiciel relevé, en attente d'être joint à un lot. Vidé
    /// quand le serveur l'a accepté.
    inventory: PendingInventory,
    /// Tâche de fond du relevé quotidien, lancée au premier cycle.
    inventory_task: Option<tokio::task::JoinHandle<()>>,
    /// Moniteurs déclarés par les étiquettes Docker au dernier cycle ; `None` :
    /// Docker illisible ou désactivé, le serveur ne doit alors rien archiver.
    declared: Option<Vec<dumbmonit_proto::DeclaredMonitor>>,
    /// Compteurs de performance Windows (liste libre, jeu MDaemon). Muet
    /// ailleurs, et muet sous Windows quand rien n'est configuré ni détecté.
    perf_counters: PerfCountersProbe,
    /// MDaemon et SecurityGateway : services, files, version (Windows).
    mail_server: MailServerProbe,
    /// Journal d'événements Windows : les règles reçues du serveur. Muet tant
    /// qu'il n'y en a aucune, c'est-à-dire partout ailleurs que sous Windows.
    eventlog: EventLogProbe,
    buffer: PendingBuffer,
    client: PushClient,
    backoff: Backoff,
    /// Commandes du serveur : redémarrage ou mise à jour d'un conteneur.
    commands: CommandRunner,
    /// Boucle du relais, lancée après le premier lot accepté — avant, le
    /// serveur ne nous connaît pas encore et refuserait la demande —, ou dès
    /// le démarrage pour un relais qui l'était à son dernier arrêt.
    relay: Option<tokio::task::JoinHandle<()>>,
    /// Tampon disque des résultats du relais, ouvert avec lui.
    relay_backlog: Option<Backlog>,
    /// Mode relais réglé depuis l'interface, tel que le dernier accusé de
    /// réception le porte. `None` tant que le serveur n'en a rien dit (serveur
    /// antérieur) : seule la configuration locale compte alors.
    server_relay: Option<RelaySetting>,
    /// Passerelle du relais pour les agents de son site, quand elle écoute.
    gateway: Option<GatewayTask>,
    /// Dernière adresse que la passerelle n'a pas pu ouvrir, et pourquoi :
    /// l'échec n'est journalisé qu'une fois, la tentative reprend à chaque lot,
    /// et la cause part au serveur ([`AgentIdentity::relay_gateway_state`]).
    gateway_failed: Option<(std::net::SocketAddr, String)>,
    /// Dernier essai de la passerelle à l'adresse du site, rempli par sa tâche
    /// et joint aux lots ([`AgentIdentity::relay_gateway_check`]).
    gateway_check: GatewayCheckSlot,
    /// Essai en cours, s'il y en a un.
    gateway_check_task: Option<tokio::task::JoinHandle<()>>,
    /// Adresse du dernier essai lancé, et quand.
    gateway_checked: Option<(String, Instant)>,
    /// Collecte de journaux de cette machine, et le réglage que le dernier accusé
    /// de réception portait (`None` : serveur antérieur, rien n'est collecté).
    logs: LogRunner,
    server_logs: Option<dumbmonit_proto::LogsSetting>,
    /// Mode sentinelle : la tâche de surveillance du serveur, et le réglage que
    /// le dernier accusé de réception portait. Elle survit à une panne du
    /// serveur — c'est son rôle —, mais pas à un redémarrage de l'agent.
    sentinel: SentinelRunner,
    /// Contrôles personnalisés confiés par le serveur, et leurs derniers
    /// verdicts en attente d'un lot accepté.
    scripts: crate::scripts::ScriptRunner,
    server_sentinel: Option<dumbmonit_proto::SentinelSetting>,
    /// Période courante : celle de la configuration, jusqu'à ce que le serveur en
    /// demande une autre dans son accusé de réception.
    interval: Duration,
    /// Secret de liaison de cette machine : ce qui prouve au serveur que c'est
    /// bien elle qui parle, et pas une voisine porteuse du même jeton de parc.
    binding: Binding,
    /// Vrai une fois le refus de liaison signalé, pour ne le dire qu'une fois
    /// par panne plutôt qu'à chaque tentative.
    binding_refused: bool,
    /// Vrai tant que le serveur fait attendre l'enrôlement de cette machine
    /// (validation par un administrateur) : dit une fois, pas à chaque essai.
    awaiting_approval: bool,
    /// Modules décochés dans l'interface, tels que le dernier accusé de
    /// réception les nomme : leur collecte est sautée. Le serveur écarte de
    /// toute façon leurs mesures ; ne pas les lire épargne la machine (un test
    /// de débit consomme de la bande passante, une vérification d'images
    /// interroge les registres).
    disabled_modules: BTreeSet<String>,
    /// Taille maximale d'un lot. Part de [`MAX_BATCH_SAMPLES`] et se réduit de
    /// moitié à chaque 413 : un mandataire au plafond de corps plus bas que le
    /// serveur ne bouge pas, inutile de revenir buter dessus.
    batch_limit: usize,
    /// Vrai une fois signalé qu'un lot a été entièrement écarté par le serveur,
    /// pour ne le dire qu'une fois par épisode.
    all_refused: bool,
    /// Début de la coupure en cours et dernière cause journalisée : la reprise
    /// dit combien de temps a duré la panne, un changement de cause est signalé.
    outage: Option<(Instant, String)>,
    /// Canal chiffré : URL, empreintes, certificat. `None` tant que ni la
    /// configuration ni le serveur n'en ont parlé (serveur antérieur).
    tls: Option<TlsState>,
    /// Où il est rangé, à côté du secret de liaison.
    tls_path: std::path::PathBuf,
    /// Pas de nouvelle tentative d'enrôlement ou de renouvellement avant.
    tls_retry_at: Option<Instant>,
    /// Vrai une fois un échec du canal chiffré signalé, pour ne le dire
    /// qu'une fois par épisode.
    tls_failed: bool,
    /// Serveur en service parmi ceux de la configuration (`server_urls`) :
    /// bascule vers un secours, retour à la primaire.
    failover: Failover,
    /// Mise à jour automatique de l'agent lui-même.
    updater: SelfUpdater,
    /// Dernière offre de mise à jour du serveur, examinée après l'envoi.
    update_offer: Option<AgentUpdateOffer>,
    /// Ce qu'il reste à faire une fois la boucle arrêtée (redémarrer sur le
    /// nouveau binaire, ou rétablir l'ancien).
    after_stop: Option<AfterStop>,
}

impl Agent {
    pub fn new(config: Config) -> Result<Self> {
        let mut identity = crate::identity::detect(&config);
        let updater = SelfUpdater::new(config.self_update);
        identity.self_update = Some(updater.info());
        let binding = Binding::load(&config.secret_path);
        let client = PushClient::new(
            &config.server_url,
            &config.token,
            binding.secret().map(str::to_string),
            HTTP_TIMEOUT,
        )
        .context("preparing the push client")?;
        // Mandataire vers le serveur, avant tout canal : chacun le reprend.
        let proxy = config.proxy.resolve();
        info!(proxy = proxy.describe(), "outbound proxy to the server");
        let client = client.with_proxy(proxy).context("configuring the outbound proxy")?;
        if let Some(pin) = &config.server_pin {
            client.pin_server(pin).context("pinning the server key")?;
        }
        // Transport du canal chiffré : la WebSocket vit à l'adresse de
        // l'interface, jamais en clair hors de la boucle locale.
        let plain = config.plain_server_url;
        let ws_url = crate::ws::websocket_url(&config.server_url).filter(|_| !plain);
        client
            .set_transport(config.transport, ws_url, config.server_pin.as_deref())
            .context("preparing the WebSocket transport")?;
        let tls_path = crate::tls::beside(&config.secret_path);
        let mut tls = initial_tls(&config, TlsState::load(&tls_path));
        if config.plain_server_url {
            plain_url_channel(&config.server_url, tls.as_mut())?;
        }
        if let Some(state) = &tls {
            client.remember_route(state.transport);
            client.set_pins(state.pins.clone());
            if let Some(identity) = state.identity() {
                client
                    .use_tls(&state.url, Some(identity))
                    .context("preparing the agent channel")?;
            } else if state.locked || config.ca_pin.is_some() {
                // Pas encore (ou plus) de certificat, mais l'agent ne parle
                // qu'au canal chiffré : rien ne partira en clair d'ici là.
                client.use_tls(&state.url, None).context("preparing the agent channel")?;
            }
        }
        let commands = CommandRunner::new(
            config.commands,
            client.clone(),
            identity.key(),
            &config.docker_socket,
        )
        .with_actions(config.actions.clone());
        let sentinel = SentinelRunner::new(
            client.clone(),
            identity.hostname.clone(),
            config.secret_path.with_file_name("sentinel.json"),
        );
        let logs = LogRunner::new(
            client.clone(),
            identity.key(),
            config.secret_path.with_file_name("logs-spool.jsonl"),
        );
        Ok(Self {
            interval: config.interval,
            buffer: PendingBuffer::new(config.max_buffered_samples)
                .with_spool(open_sample_spool(&config)),
            probe: SystemProbe::new(&config.probe),
            health: SystemHealthProbe::new(&config.system_health),
            docker: DockerProbe::new(&config.docker_socket, config.docker_max_containers),
            updates: UpdateChecker::new(),
            plakar: PlakarProbe::new(&config.plakar),
            backup_repos: BackupReposProbe::new(&config.backup_repos),
            databases: DatabasesProbe::new(&config.databases),
            wireguard: WireguardProbe::new(&config.wireguard),
            ipsec: IpsecProbe::new(&config.ipsec),
            speedtest: SpeedtestProbe::new(&config.speedtest),
            sensors: SensorsProbe::new(config.sensors),
            machine: MachineProbe::new(),
            smart: SmartProbe::new(&config.smart),
            zfs: ZfsProbe::new(&config.zfs),
            libvirt: LibvirtProbe::new(&config.libvirt),
            dsm: DsmProbe::default(),
            inventory: PendingInventory::default(),
            inventory_task: None,
            declared: None,
            perf_counters: PerfCountersProbe::for_platform(&config.perf_counters),
            mail_server: MailServerProbe::for_platform(config.perf_counters.mdaemon),
            eventlog: EventLogProbe::for_platform(
                config.secret_path.with_file_name(eventlog::STATE_FILE),
            ),
            backoff: Backoff::new(BACKOFF_BASE, BACKOFF_MAX),
            commands,
            relay: None,
            relay_backlog: None,
            server_relay: None,
            gateway: None,
            gateway_failed: None,
            gateway_check: GatewayCheckSlot::default(),
            gateway_check_task: None,
            gateway_checked: None,
            logs,
            server_logs: None,
            sentinel,
            server_sentinel: None,
            scripts: crate::scripts::ScriptRunner::new(config.scripts.clone()),
            binding,
            binding_refused: false,
            awaiting_approval: false,
            disabled_modules: BTreeSet::new(),
            batch_limit: MAX_BATCH_SAMPLES,
            all_refused: false,
            outage: None,
            tls,
            tls_path,
            tls_retry_at: None,
            tls_failed: false,
            failover: Failover::new(&config),
            updater,
            update_offer: None,
            after_stop: None,
            identity,
            client,
            config,
        })
    }

    /// Un cycle complet de mesure.
    pub async fn collect(&mut self) -> Vec<Sample> {
        let started = Instant::now();
        let mut snapshot = self.probe.read();
        snapshot.services = services::probe(&self.config.services).await;
        self.declared = None;
        if self.config.docker && self.wants("docker") {
            snapshot.containers = self.docker.read().await;
            if let Some(inventory) = snapshot.containers.as_mut() {
                self.check_updates(&mut inventory.detailed);
                self.declared = Some(inventory.declared.clone());
            }
        }
        snapshot.system_health = self.health.read().await;
        if self.wants("plakar") {
            snapshot.backups = self.plakar.read().await;
        }
        if self.wants("backup_repos") {
            snapshot.backup_repos = self.backup_repos.read().await;
        }
        if self.wants("databases") {
            snapshot.databases = self.databases.read().await;
        }
        if self.wants("wireguard") {
            snapshot.wireguard = self.wireguard.read().await;
        }
        if self.wants("ipsec") {
            snapshot.ipsec = self.ipsec.read().await;
        }
        let speedtest = self.wants("speedtest");
        if speedtest {
            snapshot.speedtest = self.speedtest.read().await;
        }
        snapshot.sensors = self.sensors.read();
        snapshot.machine = self.machine.read(&snapshot.filesystems).await;
        snapshot.smart = self.smart.read().await;
        if self.wants("zfs") {
            snapshot.zfs = self.zfs.read().await;
        }
        if self.wants("libvirt") {
            snapshot.libvirt = self.libvirt.read().await;
        }
        snapshot.dsm = self.dsm.read().await;
        self.sync_inventory();
        snapshot.perf_counters = self.perf_counters.read();
        snapshot.mail_server = self.mail_server.read().await;

        // Un seul horodatage pour tout le cycle : c'est ce qui rend les séries
        // comparables entre elles à l'instant près.
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.eventlog.read(now_ms);
        let mut samples = snapshot.to_samples(now_ms);
        if speedtest {
            samples.extend(crate::collect::speedtest::threshold_samples(
                &self.config.speedtest,
                now_ms,
            ));
        }
        samples.extend(agent_samples(
            started.elapsed(),
            self.buffer.len(),
            self.buffer.dropped(),
            now_ms,
        ));
        samples.extend(self.buffer_samples(now_ms));
        samples
    }

    /// Ce que le tampon disque occupe, et ce que le relais y garde.
    fn buffer_samples(&self, now_ms: i64) -> Vec<Sample> {
        let mut samples = Vec::new();
        if self.buffer.has_spool() {
            let bytes = self.buffer.disk_bytes() as f64;
            samples.push(Sample::new("agent_buffer_disk_bytes", bytes, MetricKind::Gauge, now_ms));
        }
        if let Some(backlog) = &self.relay_backlog {
            let stats = backlog.stats();
            let pending = stats.pending() as f64;
            let dropped = stats.dropped() as f64;
            samples.extend([
                Sample::new("agent_relay_buffered_results", pending, MetricKind::Gauge, now_ms),
                Sample::new("agent_relay_dropped_results", dropped, MetricKind::Counter, now_ms),
            ]);
        }
        samples
    }

    fn wants_inventory(&self) -> bool {
        self.config.inventory && self.wants("inventory")
    }

    /// Lance la tâche de relevé quotidien au premier cycle, l'arrête (et oublie
    /// ce qu'elle avait relevé) quand l'inventaire est coupé, en local ou depuis
    /// l'interface.
    fn sync_inventory(&mut self) {
        if self.wants_inventory() {
            if self.inventory_task.is_none() {
                self.inventory_task = Some(inventory::spawn(self.inventory.clone()));
            }
        } else if let Some(task) = self.inventory_task.take() {
            task.abort();
            *inventory::lock(&self.inventory) = None;
            info!("software inventory switched off");
        }
    }

    /// Oublie l'inventaire en attente s'il est encore celui qui vient d'être
    /// envoyé (le relevé suivant a pu le remplacer entre-temps).
    fn forget_inventory(&self, sent: Option<&SoftwareInventory>) {
        let Some(sent) = sent else { return };
        let mut pending = inventory::lock(&self.inventory);
        if pending.as_ref().is_some_and(|p| p.collected_at_ms == sent.collected_at_ms) {
            *pending = None;
        }
    }

    /// Faux pour un module que l'utilisateur a décoché dans l'interface.
    fn wants(&self, module: &str) -> bool {
        !self.disabled_modules.contains(module)
    }

    /// Adopte la liste des modules décochés que le serveur vient de renvoyer.
    fn adopt_disabled_modules(&mut self, disabled: Vec<String>) {
        let disabled: BTreeSet<String> = disabled.into_iter().collect();
        if disabled != self.disabled_modules {
            info!(
                modules = disabled.iter().map(String::as_str).collect::<Vec<_>>().join(","),
                "modules switched off from the server"
            );
            self.disabled_modules = disabled;
        }
    }

    /// Complète chaque conteneur avec le dernier verdict de mise à jour connu et
    /// relance, s'il y a lieu, la vérification de fond. Jamais d'attente ici.
    fn check_updates(&self, containers: &mut [crate::collect::docker::ContainerStat]) {
        if !self.config.docker_update_check {
            return;
        }
        let images: Vec<(String, Vec<String>)> = containers
            .iter()
            .map(|c| (c.image.clone(), self.docker.repo_digests(&c.image_id)))
            .collect();
        self.updates.refresh(images.clone());
        for (container, (image, digests)) in containers.iter_mut().zip(&images) {
            container.update_available = self.updates.result(image, digests);
        }
    }

    /// Collecte et envoie une seule fois. Sert au `--once` de vérification
    /// d'installation : le script sait ainsi tout de suite si le jeton est bon.
    pub async fn run_once(&mut self) -> Result<()> {
        self.probe.warm_up().await;
        let samples = self.collect().await;
        let count = samples.len();
        self.buffer.push(samples);
        let mut outcome = self.flush().await;
        // Vérification d'installation : le port des agents injoignable, la
        // WebSocket de l'interface est essayée tout de suite (`transport: auto`).
        if matches!(outcome, FlushOutcome::Failed(PushError::Transport(_)))
            && self.consider_transport(1).await
        {
            outcome = self.flush().await;
        }
        match outcome {
            FlushOutcome::Sent { .. } => {
                info!(samples = count, "batch sent");
                self.commands.poll().await;
                self.commands.finish(ONCE_COMMAND_TIMEOUT).await;
                Ok(())
            }
            FlushOutcome::Failed(error) => Err(anyhow::anyhow!("{error}")),
            FlushOutcome::Idle => Ok(()),
        }
    }

    pub async fn run(mut self, mut shutdown: Shutdown) -> Result<()> {
        info!(
            server = self.client.endpoint(),
            transport = self.client.describe_transport(),
            host = self.identity.hostname,
            period_s = self.interval.as_secs(),
            "agent started"
        );
        match &self.config.modules {
            Some(modules) if modules.is_empty() => {
                info!("modules: none (system metrics only)")
            }
            Some(modules) => info!(
                modules = modules.iter().map(String::as_str).collect::<Vec<_>>().join(","),
                "modules chosen in the configuration"
            ),
            None => info!("modules: auto-detected (set 'modules' in agent.yaml to choose)"),
        }
        self.restore_pending();
        // Relais au dernier arrêt : il reprend tout de suite, sans attendre
        // un serveur peut-être injoignable — c'est tout l'intérêt du planning
        // gardé. Le premier accusé de réception dira s'il doit le rester.
        if self.config.buffer_max_mb > 0 && self.plan_store().exists() {
            info!("this agent was a relay when it stopped: relaying resumes right away");
            self.start_relay(&shutdown);
        }
        // La sentinelle repart seule, sans attendre le serveur (peut-être en panne).
        self.sentinel.resume(&shutdown);
        self.probe.warm_up().await;

        let mut next_collect = Instant::now();
        let mut next_send = Instant::now();
        // Nouveau binaire à l'essai : sans lot accepté d'ici là, l'ancien
        // revient.
        let trial_deadline = self.updater.trial_deadline();
        if trial_deadline.is_some() {
            info!("new agent version on trial until its first accepted batch");
        }
        // Toute réponse favorable du serveur, y compris sur le canal du relais,
        // met fin à la temporisation : inutile d'attendre qu'elle expire.
        let reachable = self.client.reachable();
        // Lot demandé sans attendre la période (réglage du relais changé côté
        // serveur, passerelle ouverte ou essayée).
        let report_now = self.client.report_requested();

        loop {
            tokio::select! {
                // Priorité stricte à l'arrêt : sans cela, un tampon plein
                // enchaînerait les envois et repousserait indéfiniment la sortie.
                biased;

                _ = shutdown.wait() => break,

                _ = tokio::time::sleep_until(next_collect) => {
                    let samples = self.collect().await;
                    debug!(count = samples.len(), "collection cycle done");
                    self.buffer.push(samples);
                    next_collect = Instant::now() + self.interval;
                    // `next_send` n'est volontairement pas touché : s'il est déjà
                    // échu, la branche d'envoi partira au prochain tour ; s'il est
                    // dans le futur, c'est qu'une temporisation court, et une
                    // nouvelle mesure n'est pas une raison de harceler un serveur
                    // qu'on sait injoignable.
                }

                _ = tokio::time::sleep_until(next_send), if !self.buffer.is_empty() => {
                    // Sur un serveur de secours : la primaire est-elle revenue ?
                    self.check_primary().await;
                    next_send = match self.flush().await {
                        // Reste-t-il du retard ? On enchaîne, lot après lot.
                        FlushOutcome::Sent { .. } => {
                            // Le serveur vient de nous répondre : c'est le moment
                            // de lui demander s'il attend quelque chose de nous.
                            self.commands.poll().await;
                            self.sync_relay(&shutdown);
                            self.sync_gateway(&shutdown).await;
                            self.logs.apply(self.server_logs.clone(), &shutdown);
                            self.sentinel.apply(self.server_sentinel.clone(), &shutdown);
                            if self.apply_update().await {
                                break;
                            }
                            if self.buffer.catching_up() {
                                Instant::now() + CATCHUP_PAUSE
                            } else {
                                Instant::now()
                            }
                        }
                        FlushOutcome::Idle => Instant::now(),
                        FlushOutcome::Failed(error) => {
                            // Juste basculé : la nouvelle adresse est essayée
                            // sans attendre.
                            let delay = if self.failover.take_switched() {
                                Duration::ZERO
                            } else {
                                self.backoff.next_delay()
                            };
                            self.note_failure(&error, delay);
                            // La panne commence peut-être : ce qui attend passe
                            // sur le disque, et la suite y ira au fil de l'eau.
                            self.buffer.spill();
                            let unreachable = matches!(error, PushError::Transport(_));
                            if unreachable
                                && self.consider_transport(TRANSPORT_FALLBACK_AFTER).await
                            {
                                Instant::now()
                            } else {
                                Instant::now() + delay
                            }
                        }
                    };
                }

                _ = reachable.notified(), if self.outage.is_some() && !self.buffer.is_empty() => {
                    debug!("server answered on another channel, sending now");
                    next_send = Instant::now();
                }

                _ = report_now.notified() => {
                    // Une mesure fraîche, puis un lot : son accusé de réception
                    // porte le nouveau réglage, et le serveur voit la passerelle.
                    debug!("report requested, collecting and sending now");
                    next_collect = Instant::now();
                    next_send = Instant::now();
                }

                _ = tokio::time::sleep_until(trial_deadline.unwrap_or(next_collect)),
                    if trial_deadline.is_some() && self.updater.in_trial() => {
                    if let Some(action) = self.updater.expire_trial() {
                        self.after_stop = Some(action);
                        break;
                    }
                }
            }
        }

        self.commands.finish(SHUTDOWN_COMMAND_TIMEOUT).await;
        if let Some(gateway) = self.gateway.take() {
            // Les tunnels en cours se ferment avec elle : les agents du site se
            // reconnecteront d'eux-mêmes au retour du relais.
            gateway.handle.abort();
        }
        if let Some(relay) = self.relay.take() {
            if self.after_stop.is_some() {
                // Arrêt pour la mise à jour : le signal d'arrêt n'a pas été
                // donné, le relais ne rendrait pas la main de lui-même.
                relay.abort();
            } else {
                // Le relais observe le même signal d'arrêt : on attend
                // seulement qu'il ait rendu la main.
                let _ = relay.await;
            }
        }
        self.logs.finish(self.after_stop.is_none()).await;
        self.final_flush().await;
        if let Some(action) = self.after_stop.take() {
            info!("agent stopped for its update");
            return crate::selfupdate::after_stop(action);
        }
        info!("agent stopped");
        Ok(())
    }

    /// Examine l'offre de mise à jour du dernier accusé de réception. Vrai si
    /// le nouveau binaire est en place : la boucle s'arrête pour lui céder la
    /// place.
    async fn apply_update(&mut self) -> bool {
        let Some(offer) = self.update_offer.take() else { return false };
        let key = self.identity.key();
        let after = self.updater.consider(&offer, &self.client, &key).await;
        // L'issue (refus, échec) part avec le lot suivant.
        self.identity.self_update = Some(self.updater.info());
        match after {
            Some(action) => {
                self.after_stop = Some(action);
                true
            }
            None => false,
        }
    }
}

/// Passerelle du relais en service.
struct GatewayTask {
    /// Adresse demandée (comparée au réglage en vigueur).
    addr: std::net::SocketAddr,
    /// Adresse réellement liée, annoncée au serveur.
    bound: std::net::SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

#[derive(Debug)]
enum FlushOutcome {
    /// Rien à envoyer.
    Idle,
    Sent {
        count: usize,
    },
    Failed(PushError),
}
