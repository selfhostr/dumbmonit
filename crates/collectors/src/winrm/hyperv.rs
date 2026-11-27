//! Hôte Hyper-V sans agent : WinRM et requêtes WQL sur `root/virtualization/v2`.
//!
//! Même transport, mêmes comptes et mêmes réglages que [`super`] (WinRM, NTLM ou
//! Basic, HTTPS par défaut) ; seules changent les classes lues. Aucune commande
//! n'est exécutée sur l'hôte : des `SELECT` fixes sur les classes `Msvm_*`.
//!
//! # Ce qui est mesuré
//!
//! * l'hôte : charge processeur de l'hyperviseur, mémoire, uptime, nombre de
//!   machines virtuelles (`hyperv_host_*`, `hyperv_vms`, `hyperv_vms_running`) ;
//! * chaque machine virtuelle (étiquette `vm`) : état, santé, uptime, charge
//!   processeur, processeurs, mémoire attribuée, services d'intégration
//!   (battement de cœur), réplication, points de contrôle (nombre et âge du plus
//!   ancien).
//!
//! # Lectures facultatives
//!
//! La liste des machines est la seule lecture obligatoire. Le résumé
//! (`Msvm_SummaryInformation`), les points de contrôle, la charge de l'hôte et le
//! système d'exploitation sont facultatifs : refusés ou absents, la série
//! correspondante manque, et `winrm_section_readable{section}` le dit.
//!
//! Hôte sans le rôle Hyper-V : l'espace de noms n'existe pas, la sonde rend une
//! erreur de configuration (affichée, non notifiée) et non une panne.

use std::collections::BTreeMap;

use async_trait::async_trait;
use dumbmonit_proto::{Collector, ProbeError, Sample, Target};

use super::wsman::Row;
use super::{
    MAX_PAGES, OS_WQL, Options, PAGE_SIZE, Session, credentials, number, parse_wmi_time, percent,
};
use crate::rest::{MAX_NAMED, flag, gauge, now_ms};

/// Espace de noms WMI de Hyper-V (Windows Server 2012 R2 et suivants).
const NAMESPACE: &str = "root/virtualization/v2";
const CIMV2: &str = super::wsman::CIMV2;

const VMS_WQL: &str = "SELECT Name, ElementName, EnabledState, HealthState, OnTimeInMilliseconds, \
    ReplicationState, ReplicationHealth FROM Msvm_ComputerSystem WHERE Caption = 'Virtual Machine'";
const SUMMARY_WQL: &str = "SELECT Name, NumberOfProcessors, ProcessorLoad, MemoryUsage, Heartbeat \
    FROM Msvm_SummaryInformation";
const CHECKPOINTS_WQL: &str = "SELECT VirtualSystemIdentifier, CreationTime \
    FROM Msvm_VirtualSystemSettingData \
    WHERE VirtualSystemType = 'Microsoft:Hyper-V:Snapshot:Realized'";
const HOST_CPU_WQL: &str = "SELECT PercentTotalRunTime FROM \
    Win32_PerfFormattedData_HvStats_HyperVHypervisorLogicalProcessor WHERE Name = '_Total'";

/// `EnabledState` d'une machine en marche.
const STATE_RUNNING: f64 = 2.0;
/// `HealthState` « OK ».
const HEALTH_OK: f64 = 5.0;
/// `Heartbeat` (OperationalStatus) « OK » : le service d'intégration répond.
const HEARTBEAT_OK: f64 = 2.0;
/// `ReplicationHealth` « Normal » (0 : réplication non configurée).
const REPLICATION_NORMAL: f64 = 1.0;

#[derive(Default)]
pub struct HypervCollector;

impl HypervCollector {
    pub fn new() -> Self {
        Self
    }
}

/// Ce que les requêtes ont rendu ; `None` = section illisible.
pub struct Replies {
    pub vms: Vec<Row>,
    pub summary: Option<Vec<Row>>,
    pub checkpoints: Option<Vec<Row>>,
    pub host_cpu: Option<Vec<Row>>,
    pub os: Option<Vec<Row>>,
}

#[async_trait]
impl Collector for HypervCollector {
    fn kind(&self) -> &'static str {
        "hyperv"
    }

    async fn probe(&self, target: &Target) -> Result<Vec<Sample>, ProbeError> {
        let options = Options::from_target(target)?;
        let (username, password) = credentials(target)?;
        let mut session = Session::new(options, username, password)?;
        let vms = match session.query_in(NAMESPACE, VMS_WQL, PAGE_SIZE, MAX_PAGES).await {
            Ok(vms) => vms,
            // Une faute SOAP (espace de noms absent) n'est pas une panne de l'hôte.
            Err(ProbeError::Protocol(detail)) => {
                return Err(ProbeError::Config(format!(
                    "Hyper-V is not readable on this host (is the role installed, and the \
                     account allowed on root\\virtualization\\v2?): {detail}"
                )));
            }
            Err(error) => return Err(error),
        };
        let replies = Replies {
            vms,
            summary: optional(&mut session, NAMESPACE, SUMMARY_WQL).await?,
            checkpoints: optional(&mut session, NAMESPACE, CHECKPOINTS_WQL).await?,
            host_cpu: optional(&mut session, CIMV2, HOST_CPU_WQL).await?,
            os: optional(&mut session, CIMV2, OS_WQL).await?,
        };
        Ok(samples(&replies, now_ms()))
    }

    async fn discover(&self, target: &Target) -> Result<Option<String>, ProbeError> {
        let options = Options::from_target(target)?;
        let (username, password) = credentials(target)?;
        let mut session = Session::new(options, username, password)?;
        session.query_in(NAMESPACE, VMS_WQL, 1, 1).await?;
        Ok(Some("hyperv".to_string()))
    }
}

/// Lecture facultative : refusée ou en erreur de requête, elle vaut `None` ; une
/// panne réelle (injoignable, délai) fait échouer toute l'interrogation.
async fn optional(
    session: &mut Session,
    namespace: &str,
    wql: &str,
) -> Result<Option<Vec<Row>>, ProbeError> {
    match session.query_in(namespace, wql, PAGE_SIZE, MAX_PAGES).await {
        Ok(rows) => Ok(Some(rows)),
        Err(error) if error.means_down() => Err(error),
        Err(error) => {
            tracing::debug!(%error, wql, "Hyper-V : lecture facultative indisponible");
            Ok(None)
        }
    }
}

/// Nom affiché d'une machine : son nom, suivi du début de son identifiant si un
/// autre porte le même (les noms ne sont pas uniques dans Hyper-V).
fn display_names(vms: &[Row]) -> BTreeMap<String, String> {
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for vm in vms {
        *count.entry(vm.get("ElementName").map_or("", String::as_str)).or_default() += 1;
    }
    vms.iter()
        .filter_map(|vm| {
            let id = vm.get("Name")?;
            let name = vm.get("ElementName").map_or("", String::as_str);
            let shown = if name.is_empty() || count[name] > 1 {
                format!("{name} ({})", id.chars().take(8).collect::<String>())
            } else {
                name.to_string()
            };
            Some((id.clone(), shown))
        })
        .collect()
}

pub fn samples(r: &Replies, ts_ms: i64) -> Vec<Sample> {
    let g = |name: &str, value: f64| gauge(name, value, ts_ms);
    let now_s = ts_ms as f64 / 1000.0;
    let mut out = Vec::new();

    let running = r.vms.iter().filter(|vm| number(vm, "EnabledState") == Some(STATE_RUNNING));
    out.push(g("hyperv_vms", r.vms.len() as f64));
    out.push(g("hyperv_vms_running", running.count() as f64));

    // Hôte.
    if let Some(os) = r.os.as_ref().and_then(|rows| rows.first()) {
        // Les tailles WMI sont en kilo-octets.
        if let (Some(total), Some(free)) =
            (number(os, "TotalVisibleMemorySize"), number(os, "FreePhysicalMemory"))
        {
            let (total, used) = (total * 1024.0, ((total - free) * 1024.0).max(0.0));
            out.push(g("hyperv_host_memory_total_bytes", total));
            out.push(g("hyperv_host_memory_used_bytes", used));
            out.push(g("hyperv_host_memory_used_percent", percent(used, total)));
        }
        if let Some(boot) = os.get("LastBootUpTime").and_then(|t| parse_wmi_time(t)) {
            out.push(g("hyperv_host_uptime_seconds", (now_s - boot as f64).max(0.0)));
        }
    }
    if let Some(cpu) = r.host_cpu.as_ref().and_then(|rows| rows.first())
        && let Some(load) = number(cpu, "PercentTotalRunTime")
    {
        out.push(g("hyperv_host_cpu_percent", load.clamp(0.0, 100.0)));
    }

    let names = display_names(&r.vms);
    let summary: BTreeMap<&str, &Row> = r
        .summary
        .iter()
        .flatten()
        .filter_map(|row| Some((row.get("Name")?.as_str(), row)))
        .collect();
    // Points de contrôle par machine : nombre et plus ancien.
    let mut checkpoints: BTreeMap<&str, (f64, i64)> = BTreeMap::new();
    for row in r.checkpoints.iter().flatten() {
        let Some(id) = row.get("VirtualSystemIdentifier") else { continue };
        let created = row.get("CreationTime").and_then(|t| parse_wmi_time(t)).unwrap_or(i64::MAX);
        let entry = checkpoints.entry(id.as_str()).or_insert((0.0, i64::MAX));
        entry.0 += 1.0;
        entry.1 = entry.1.min(created);
    }

    for vm in r.vms.iter().take(MAX_NAMED) {
        let Some(id) = vm.get("Name") else { continue };
        let name = names[id].as_str();
        let labelled = |metric: &str, value: f64| g(metric, value).with_label("vm", name);

        let state = number(vm, "EnabledState");
        if let Some(state) = state {
            out.push(labelled("hyperv_vm_state", state));
            let up = flag("hyperv_vm_running", state == STATE_RUNNING, ts_ms);
            out.push(up.with_label("vm", name));
        }
        if let Some(health) = number(vm, "HealthState") {
            out.push(
                flag("hyperv_vm_health_ok", health == HEALTH_OK, ts_ms).with_label("vm", name),
            );
        }
        let is_running = state == Some(STATE_RUNNING);
        if is_running && let Some(ms) = number(vm, "OnTimeInMilliseconds") {
            out.push(labelled("hyperv_vm_uptime_seconds", ms / 1000.0));
        }

        if let Some(info) = summary.get(id.as_str()) {
            if is_running && let Some(load) = number(info, "ProcessorLoad") {
                out.push(labelled("hyperv_vm_cpu_percent", load.clamp(0.0, 100.0)));
            }
            if let Some(cpus) = number(info, "NumberOfProcessors") {
                out.push(labelled("hyperv_vm_cpus", cpus));
            }
            // `MemoryUsage` : mégaoctets attribués à la machine.
            if is_running && let Some(mb) = number(info, "MemoryUsage") {
                out.push(labelled("hyperv_vm_memory_assigned_bytes", mb * 1024.0 * 1024.0));
            }
            // 0 : service d'intégration inconnu, rien à dire ; sinon 2 = répond.
            if is_running
                && let Some(beat) = number(info, "Heartbeat").filter(|beat| *beat != 0.0)
            {
                out.push(
                    flag("hyperv_vm_integration_ok", beat == HEARTBEAT_OK, ts_ms)
                        .with_label("vm", name),
                );
            }
        }

        // Réplication : 0 = non configurée, aucune série.
        if let Some(health) = number(vm, "ReplicationHealth").filter(|health| *health != 0.0) {
            out.push(
                flag("hyperv_vm_replication_ok", health == REPLICATION_NORMAL, ts_ms)
                    .with_label("vm", name),
            );
        }

        // Une machine sans point de contrôle l'annonce (zéro) quand la lecture a
        // réussi : l'alerte « point de contrôle ancien » se lève et se résout.
        if r.checkpoints.is_some() {
            let (count, oldest) = checkpoints.get(id.as_str()).copied().unwrap_or((0.0, i64::MAX));
            out.push(labelled("hyperv_vm_checkpoints", count));
            if count > 0.0 && oldest != i64::MAX {
                out.push(labelled(
                    "hyperv_vm_checkpoint_oldest_age_seconds",
                    (now_s - oldest as f64).max(0.0),
                ));
            }
        }
    }

    for (section, rows) in [
        ("hyperv_summary", &r.summary),
        ("hyperv_checkpoints", &r.checkpoints),
        ("hyperv_host_cpu", &r.host_cpu),
        ("hyperv_os", &r.os),
    ] {
        let readable = flag("winrm_section_readable", rows.is_some(), ts_ms);
        out.push(readable.with_label("section", section));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_792_000_000_000;

    fn row(pairs: &[(&str, &str)]) -> Row {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn vm(id: &str, name: &str, state: &str) -> Row {
        row(&[
            ("Name", id),
            ("ElementName", name),
            ("EnabledState", state),
            ("HealthState", "5"),
            ("OnTimeInMilliseconds", "7200000"),
            ("ReplicationHealth", "0"),
        ])
    }

    fn replies(vms: Vec<Row>) -> Replies {
        Replies { vms, summary: None, checkpoints: None, host_cpu: None, os: None }
    }

    fn value(samples: &[Sample], metric: &str, vm: &str) -> Option<f64> {
        samples
            .iter()
            .find(|s| s.metric == metric && s.labels.get("vm").is_none_or(|v| v == vm))
            .map(|s| s.value)
    }

    #[test]
    fn une_machine_en_marche_donne_etat_sante_et_uptime() {
        let samples = samples(&replies(vec![vm("A1", "web", "2"), vm("B2", "db", "3")]), NOW_MS);
        assert_eq!(value(&samples, "hyperv_vms", ""), Some(2.0));
        assert_eq!(value(&samples, "hyperv_vms_running", ""), Some(1.0));
        assert_eq!(value(&samples, "hyperv_vm_running", "web"), Some(1.0));
        assert_eq!(value(&samples, "hyperv_vm_running", "db"), Some(0.0));
        assert_eq!(value(&samples, "hyperv_vm_health_ok", "web"), Some(1.0));
        assert_eq!(value(&samples, "hyperv_vm_uptime_seconds", "web"), Some(7200.0));
        // Éteinte : pas d'uptime.
        assert_eq!(value(&samples, "hyperv_vm_uptime_seconds", "db"), None);
    }

    #[test]
    fn deux_machines_homonymes_restent_distinctes() {
        let twins = vec![vm("AAAAAAAA-1", "web", "2"), vm("BBBBBBBB-2", "web", "2")];
        let samples = samples(&replies(twins), NOW_MS);
        let names: Vec<&str> = samples
            .iter()
            .filter(|s| s.metric == "hyperv_vm_state")
            .map(|s| s.labels["vm"].as_str())
            .collect();
        assert_eq!(names, vec!["web (AAAAAAAA)", "web (BBBBBBBB)"]);
    }

    #[test]
    fn le_resume_apporte_charge_memoire_et_services_d_integration() {
        let mut r = replies(vec![vm("A1", "web", "2")]);
        r.summary = Some(vec![row(&[
            ("Name", "A1"),
            ("NumberOfProcessors", "4"),
            ("ProcessorLoad", "37"),
            ("MemoryUsage", "2048"),
            ("Heartbeat", "12"),
        ])]);
        let samples = samples(&r, NOW_MS);
        assert_eq!(value(&samples, "hyperv_vm_cpu_percent", "web"), Some(37.0));
        assert_eq!(value(&samples, "hyperv_vm_cpus", "web"), Some(4.0));
        let memory = value(&samples, "hyperv_vm_memory_assigned_bytes", "web");
        assert_eq!(memory, Some(2_147_483_648.0));
        // 12 : plus de contact avec le service d'intégration.
        assert_eq!(value(&samples, "hyperv_vm_integration_ok", "web"), Some(0.0));
    }

    #[test]
    fn les_points_de_controle_sont_comptes_par_machine_avec_l_age_du_plus_ancien() {
        let mut r = replies(vec![vm("A1", "web", "2"), vm("B2", "db", "2")]);
        // Un et deux jours avant NOW_MS (2026-10-14 17:46:40 UTC).
        let day_before = "20261013174640.000000+000";
        let two_days = "20261012174640.000000+000";
        r.checkpoints = Some(vec![
            row(&[("VirtualSystemIdentifier", "A1"), ("CreationTime", day_before)]),
            row(&[("VirtualSystemIdentifier", "A1"), ("CreationTime", two_days)]),
        ]);
        let samples = samples(&r, NOW_MS);
        assert_eq!(value(&samples, "hyperv_vm_checkpoints", "web"), Some(2.0));
        assert_eq!(value(&samples, "hyperv_vm_checkpoints", "db"), Some(0.0));
        let oldest = "hyperv_vm_checkpoint_oldest_age_seconds";
        assert_eq!(value(&samples, oldest, "web"), Some(172_800.0));
        assert_eq!(value(&samples, oldest, "db"), None);
    }

    #[test]
    fn la_replication_non_configuree_ne_publie_rien() {
        let mut broken = vm("B2", "db", "2");
        broken.insert("ReplicationHealth".into(), "3".into());
        let mut ok = vm("C3", "app", "2");
        ok.insert("ReplicationHealth".into(), "1".into());
        let samples = samples(&replies(vec![vm("A1", "web", "2"), broken, ok]), NOW_MS);
        assert_eq!(value(&samples, "hyperv_vm_replication_ok", "web"), None);
        assert_eq!(value(&samples, "hyperv_vm_replication_ok", "db"), Some(0.0));
        assert_eq!(value(&samples, "hyperv_vm_replication_ok", "app"), Some(1.0));
    }

    #[test]
    fn l_hote_donne_memoire_uptime_et_charge() {
        let mut r = replies(vec![]);
        r.os = Some(vec![row(&[
            ("TotalVisibleMemorySize", "33554432"),
            ("FreePhysicalMemory", "8388608"),
            ("LastBootUpTime", "20261013174640.000000+000"),
        ])]);
        r.host_cpu = Some(vec![row(&[("PercentTotalRunTime", "42")])]);
        let samples = samples(&r, NOW_MS);
        assert_eq!(value(&samples, "hyperv_host_memory_used_percent", ""), Some(75.0));
        assert_eq!(value(&samples, "hyperv_host_uptime_seconds", ""), Some(86_400.0));
        assert_eq!(value(&samples, "hyperv_host_cpu_percent", ""), Some(42.0));
    }

    #[test]
    fn une_section_refusee_est_signalee_sans_serie() {
        let samples = samples(&replies(vec![vm("A1", "web", "2")]), NOW_MS);
        assert_eq!(value(&samples, "hyperv_vm_checkpoints", "web"), None);
        let unreadable = samples
            .iter()
            .filter(|s| s.metric == "winrm_section_readable" && s.value == 0.0)
            .count();
        assert_eq!(unreadable, 4);
    }
}
