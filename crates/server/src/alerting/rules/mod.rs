//! Règles livrées par défaut.
//!
//! Objectif produit : une instance fraîchement installée doit alerter utilement
//! sans que personne n'ait rien réglé. Ces règles sont insérées au premier
//! démarrage puis appartiennent à l'utilisateur — elles ne sont jamais réécrites
//! par-dessus ses modifications, seules les règles manquantes sont recréées.

use std::time::Duration;

use crate::alerting::model::{
    AnomalyParams, Operator, RULE_HOST_DOWN, Rule, RuleKind, Severity, TargetSelector,
};
use crate::slo::RULE_SLO_BURN;

mod adguard;
mod builtin_apps;
mod builtin_data;
mod builtin_general;
mod builtin_infra;
mod builtin_proxmox;
mod builtin_services;
mod builtin_vendors;
mod cloud;
mod flags;
mod homeassistant;
mod hyperv;
mod immich;
mod jellyfin;
mod mysql;
mod nextcloud;
mod opnsense;
mod pbs;
mod pfsense;
mod pihole;
mod plex;
mod postgres;
mod pve;
mod selfhosted;
mod synology;
mod tailscale;
mod truenas;
mod unifi;
mod unraid;
mod veeam;
mod vsphere;

/// Taux d'occupation processeur, toutes sources confondues.
///
/// Chaque collecteur nomme la sienne : `cpu_load_percent` pour SNMP
/// (HOST-RESOURCES-MIB), `proxmox_node_cpu_percent` pour Proxmox,
/// `cpu_usage_percent` pour l'agent et la démonstration. L'union `or` fait que la
/// règle livrée fonctionne quel que soit le type d'équipement ajouté, sans que
/// l'utilisateur ait à écrire quoi que ce soit.
const CPU_PERCENT: &str = "avg by (target, host) (\
     dumbmonit_cpu_load_percent \
     or dumbmonit_proxmox_node_cpu_percent \
     or dumbmonit_cpu_usage_percent)";

/// Taux de remplissage des systèmes de fichiers.
///
/// SNMP expose des octets utilisés et totaux (HOST-RESOURCES-MIB), pas un
/// pourcentage : il est calculé ici. Proxmox fournit déjà le taux.
const FS_USED_PERCENT: &str = "(\
     100 * dumbmonit_storage_bytes_used / dumbmonit_storage_bytes_total \
     or dumbmonit_proxmox_storage_used_percent \
     or dumbmonit_proxmox_node_rootfs_percent)";

/// Âge du dernier échantillon de chaque cible, en secondes : la plus fraîche de
/// ses séries `dumbmonit_up` (voir la règle « Monitor unreachable »).
pub const HOST_DOWN_QUERY: &str =
    "bottomk by (target) (1, time() - tlast_over_time(dumbmonit_up[7d]))";

/// Part des insertions lentes de VictoriaMetrics sur quinze minutes, pour un
/// serveur démarré depuis plus longtemps que la fenêtre (voir la règle).
pub const VM_SLOW_INSERTS_QUERY: &str = "(100 \
     * increase_prometheus(dumbmonit_victoriametrics_slow_inserts_total[15m]) \
     / increase_prometheus(dumbmonit_victoriametrics_rows_added_total[15m])) \
     and on (target) dumbmonit_victoriametrics_uptime_seconds > 900";

/// Durée des sondes de disponibilité réussies (voir la règle `service_slow`).
pub const SERVICE_SLOW_QUERY: &str =
    "dumbmonit_probe_duration_seconds and dumbmonit_probe_success == 1";

/// `uid` de la règle livrée de prévision de remplissage : la route
/// `GET /api/targets/{id}/forecasts` reprend ses réglages (profondeur, paliers).
pub const RULE_CAPACITY_FORECAST: &str = "capacity_forecast";

/// Part de réponses 5xx, en pourcentage, des proxys (avec un plancher de
/// cinquante requêtes sur dix minutes) et des zones Cloudflare.
pub const HTTP_5XX_PERCENT: &str = "(100 \
     * increase_prometheus(dumbmonit_traefik_requests_5xx_total[10m]) \
     / increase_prometheus(dumbmonit_traefik_requests_total[10m]) \
     and increase_prometheus(dumbmonit_traefik_requests_total[10m]) >= 50) \
     or (100 * increase_prometheus(dumbmonit_caddy_requests_5xx_total[10m]) \
     / increase_prometheus(dumbmonit_caddy_requests_total[10m]) \
     and increase_prometheus(dumbmonit_caddy_requests_total[10m]) >= 50) \
     or dumbmonit_cloudflare_zone_http_5xx_percent";

/// Pod en boucle de redémarrage : le drapeau du collecteur, tenu par un compteur
/// de redémarrages qui progresse encore (voir la règle `k8s_pod_crashlooping`).
pub const K8S_CRASHLOOP_QUERY: &str = "max_over_time(dumbmonit_k8s_pod_crashlooping[10m]) \
     and on (target, namespace, pod) \
     increase_prometheus(dumbmonit_k8s_pod_restarts[15m]) >= 2";

/// Rappel par défaut : six heures. Assez pour ne pas oublier une panne en cours,
/// assez peu pour ne pas devenir du bruit pendant une semaine de vacances.
const DEFAULT_REPEAT: Duration = Duration::from_secs(6 * 3600);

fn base(uid: &str, name: &str, kind: RuleKind, query: &str) -> Rule {
    Rule {
        // `id = 0` : la règle n'est pas encore en base, l'insertion l'attribuera.
        id: 0,
        uid: uid.to_string(),
        name: name.to_string(),
        description: String::new(),
        kind,
        query: query.to_string(),
        operator: Operator::Gt,
        threshold: 0.0,
        // Pas d'hystérésis par défaut : chaque règle choisit son seuil de retour.
        clear_threshold: None,
        for_duration: Duration::ZERO,
        severity: Severity::Warning,
        selector: TargetSelector::All,
        // Vide signifie « tous les canaux actifs » : sans cela, les règles livrées
        // seraient muettes tant que l'utilisateur n'aurait pas pensé à les rattacher
        // au canal qu'il vient de créer.
        channels: Vec::new(),
        params: AnomalyParams::default(),
        unit: String::new(),
        repeat_interval: Some(DEFAULT_REPEAT),
        escalate_after: None,
        enabled: true,
        builtin: true,
    }
}

/// Les règles livrées avec le produit.
/// Conteneur arrêté, hors arrêts voulus que l'agent signale.
const CONTAINER_STOPPED_QUERY: &str = "dumbmonit_container_up == bool 0 \
     unless on(target, container) dumbmonit_container_stop_expected == 1";

pub fn builtin_rules() -> Vec<Rule> {
    let mut rules = builtin_general::rules();
    rules.extend(builtin_proxmox::rules());
    rules.extend(builtin_infra::rules());
    rules.extend(builtin_apps::rules());
    rules.extend(builtin_data::rules());
    rules.extend(builtin_services::rules());
    rules.extend(builtin_vendors::rules());
    // Intégrations en nuage, un fichier par produit (`rules/cloud/`).
    rules.extend(cloud::rules());
    rules.extend(pve::rules());
    rules.extend(pbs::rules());
    rules.extend(truenas::rules());
    rules.extend(unifi::rules());
    rules.extend(pfsense::rules());
    rules.extend(unraid::rules());
    rules.extend(veeam::rules());
    rules.extend(vsphere::rules());
    rules.extend(opnsense::rules());
    rules.extend(tailscale::rules());
    // Résolveurs DNS et domotique : un fichier par produit (`rules/<produit>.rs`).
    rules.extend(pihole::rules());
    rules.extend(adguard::rules());
    rules.extend(homeassistant::rules());
    rules.extend(immich::rules());
    rules.extend(jellyfin::rules());
    rules.extend(plex::rules());
    rules.extend(nextcloud::rules());
    rules.extend(hyperv::rules());
    rules.extend(mysql::rules());
    rules.extend(postgres::rules());
    rules.extend(synology::rules());
    rules.extend(selfhosted::apps_rules());
    // Les règles « tout ou rien » portent l'unité `bool` : rien à régler (`flags.rs`).
    flags::mark_flags(&mut rules);
    rules
}

/// `uid` des règles d'inventaire du réseau, livrées désactivées.
pub const RULE_NETSCAN_NEW_DEVICE: &str = "netscan_new_device";
pub const RULE_NETSCAN_NEW_PORT: &str = "netscan_new_port";
pub const RULE_NETSCAN_MAC_CHANGE: &str = "netscan_mac_change";

/// Construit une requête prédictive « ce système de fichiers sera plein dans moins
/// de `days` jours », pour l'assistant de création de règle de l'interface.
///
/// Le calcul reste intégralement côté VictoriaMetrics : rien n'est extrapolé en Rust,
/// ce qui évite d'avoir à rapatrier l'historique de chaque série à chaque cycle.
pub fn predict_full_query(metric: &str, window_hours: u32, days: u32) -> String {
    let horizon = u64::from(days) * 24 * 3600;
    format!(
        "predict_linear({metric}[{window_hours}h], {horizon}) and deriv({metric}[{window_hours}h]) > 0"
    )
}

#[cfg(test)]
mod produites;
#[cfg(test)]
pub(crate) use produites::PRODUITES;

#[cfg(test)]
mod tests;
