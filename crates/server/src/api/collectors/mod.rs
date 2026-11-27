//! Types d'équipements que l'on peut ajouter, et comment les préparer.
//!
//! L'interface découvre cette liste au lieu de la coder en dur : elle ne propose
//! donc jamais un type que ce serveur n'embarque pas, et récupère automatiquement
//! ceux qu'une version ultérieure ajoutera.
//!
//! Chaque type porte sa propre notice de mise en route. C'est ce qui permet à
//! l'interface d'afficher, à côté du formulaire, les étapes à faire sur
//! l'équipement lui-même — la moitié du travail d'ajout se passe là-bas, pas ici.
//!
//! Chaque type décrit aussi ses options : les réglages qu'un collecteur lit dans
//! les étiquettes de la cible (`Target::tags`). Sans cette liste, l'utilisateur
//! devrait deviner le nom exact d'une étiquette et sa syntaxe ; avec elle,
//! l'interface propose un champ par option, avec son défaut et son aide. La liste
//! est un miroir des `options.rs` de chaque collecteur : une étiquette absente
//! ici reste lue, mais personne ne saura qu'elle existe.
//!
//! Les tables ci-dessous ne décrivent que les types compilés. Un type né à
//! l'exécution (paquet d'intégration) se décrit lui-même
//! (`Collector::description`), et tous sont servis sous la même forme,
//! `dumbmonit_proto::KindDescription`.

use axum::Json;
use axum::extract::State;
use dumbmonit_proto::{
    CredentialDescription, CredentialFieldDescription, KindDescription, OptionColumn,
    OptionDescription, OptionItem, PortDescription, Requirement, SetupDescription, Text,
};

use crate::state::AppState;

pub struct CollectorView {
    /// Valeur à placer dans le champ `kind` d'une cible.
    pub kind: &'static str,
    /// Libellé destiné à l'affichage.
    pub label: &'static str,
    /// Une phrase disant à quoi sert ce type, pour la liste de choix.
    pub summary: &'static str,
    /// Exemples d'équipements concernés, pour que l'utilisateur se reconnaisse.
    pub examples: &'static [&'static str],
    /// Formes de `credential` acceptées (leurs `kind`), dans l'ordre de préférence.
    /// Redondant avec `credentials`, conservé pour les interfaces plus anciennes.
    pub credential_types: &'static [&'static str],
    /// Les mêmes formes, décrites champ par champ : c'est ce que le formulaire
    /// affiche. Un jeton Proxmox se saisit ainsi en deux cases (identifiant et
    /// secret) plutôt qu'en une chaîne `user@pve!nom=secret` que personne ne devine.
    pub credentials: &'static [CredentialView],
    /// Adresse d'exemple, utilisée comme texte indicatif du champ.
    pub address_hint: &'static str,
    /// Port par défaut, affiché pour lever le doute.
    pub default_port: u16,
    /// Notice de mise en route affichée à côté du formulaire.
    pub setup: Setup,
    /// Réglages lus dans les étiquettes de la cible, dans l'ordre d'affichage.
    /// Vide pour un type qui n'en lit aucune.
    pub options: &'static [OptionView],
}

/// Un port à ouvrir (voir `Setup::ports`). Les macros `to_device!`, `to_device_optional!`
/// et `to_server!` fixent le sens : `to_device` va du serveur DumbMonit (ou de l'agent
/// relais) vers l'équipement, `to_server` de l'agent vers le serveur.
pub struct PortView {
    pub port: u16,
    pub protocol: &'static str,
    pub direction: &'static str,
    pub purpose: &'static str,
    pub optional: bool,
}

// Des macros et non des fonctions : une expression `&[to_device(…)]` n'est pas promue en
// `'static` dans un `match`, alors qu'un littéral de structure l'est.
macro_rules! to_device {
    ($port:expr, $protocol:expr, $purpose:expr $(,)?) => {
        PortView {
            port: $port,
            protocol: $protocol,
            direction: "to_device",
            purpose: $purpose,
            optional: false,
        }
    };
}

macro_rules! to_device_optional {
    ($port:expr, $protocol:expr, $purpose:expr $(,)?) => {
        PortView {
            port: $port,
            protocol: $protocol,
            direction: "to_device",
            purpose: $purpose,
            optional: true,
        }
    };
}

macro_rules! to_server {
    ($port:expr, $protocol:expr, $purpose:expr $(,)?) => {
        PortView {
            port: $port,
            protocol: $protocol,
            direction: "to_server",
            purpose: $purpose,
            optional: false,
        }
    };
}

mod builders;
mod cloud;
mod credentials;
mod kinds_apps;
mod kinds_devices;
mod kinds_probes;
mod kinds_services;
mod options_apps;
mod options_devices;
mod options_probes;
mod options_scripts;
mod options_services;
mod options_vendors;

use builders::*;
use credentials::*;
use options_apps::*;
use options_devices::*;
use options_probes::*;
use options_scripts::*;
use options_services::*;
use options_vendors::*;

impl PortView {
    fn description(&self) -> PortDescription {
        PortDescription {
            port: self.port,
            protocol: self.protocol.into(),
            direction: self.direction.into(),
            purpose: self.purpose.into(),
            optional: self.optional,
        }
    }
}

pub struct Setup {
    /// Titre de la notice.
    pub title: &'static str,
    /// Étapes à effectuer sur l'équipement, dans l'ordre.
    ///
    /// Une étape est une phrase courte, faite dans l'interface web du produit
    /// chaque fois qu'il en a une. Si elle est suivie d'un saut de ligne, ce qui
    /// suit est une valeur ou, faute d'interface, une commande à copier telle
    /// quelle : l'interface l'affiche dans un bloc avec un bouton « copier », et
    /// la documentation la reprend dans un bloc de code. Les mêmes étapes
    /// figurent dans `docs/devices/<kind>.md` ; un test vérifie qu'elles n'ont
    /// pas divergé.
    pub steps: &'static [&'static str],
    /// Ports réseau à ouvrir, un par entrée ; vide si le type n'ouvre aucune connexion
    /// vers l'équipement (adresse seule, ou jeton poussé). Voir [`PortView`].
    pub ports: &'static [PortView],
    /// Droits, rôles ou permissions à donner au compte, un par entrée : la
    /// première ligne est le droit tel que le produit le nomme (affiché en gras),
    /// la suivante, facultative, dit à quoi il sert. Vide si le type n'en demande aucun.
    pub permissions: &'static [&'static str],
    /// Point d'attention fréquent, à mettre en évidence. Vide s'il n'y en a pas.
    pub warning: &'static str,
}

/// Un réglage porté par une étiquette de la cible.
///
/// C'est le contrat avec l'interface : elle construit un champ de formulaire par
/// entrée, et enregistre la valeur saisie sous la clé `key` dans `Target::tags`.
pub struct OptionView {
    /// Clé du tag dans `Target::tags`.
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub placeholder: &'static str,
    /// Valeur par défaut affichée (vide si aucune).
    pub default: &'static str,
    pub required: bool,
    /// `text`, `number`, `boolean`, `select`.
    pub input: &'static str,
    /// Valeurs proposées quand `input == "select"`.
    pub choices: &'static [&'static str],
    /// Sous-titre regroupant ce réglage avec d'autres dans « Plus d'options » ;
    /// vide par défaut (voir [`grouped`]).
    pub group: &'static str,
    /// Cases d'un réglage `checklist`, construites à l'appel (le catalogue d'un
    /// collecteur vit dans son crate, pas ici).
    pub items: fn() -> Vec<OptionItem>,
    /// Colonnes d'un réglage `rows`.
    pub columns: fn() -> Vec<OptionColumn>,
}

/// Une forme d'identifiant acceptée par un type, et les champs à remplir.
///
/// C'est le contrat avec le formulaire : un champ par entrée, envoyé sous la clé
/// `key` dans l'objet `credential` (`{"type": kind, key: valeur, …}`). Le serveur
/// sait recomposer ce qu'il attend — un jeton Proxmox à partir de `token_id` et
/// `secret`, un SNMP v3 à partir de ses protocoles et phrases de passe.
pub struct CredentialView {
    /// Valeur du champ `type` de `credential`.
    pub kind: &'static str,
    pub label: &'static str,
    /// Une phrase pour situer cette forme par rapport aux autres. Vide si inutile.
    pub help: &'static str,
    pub fields: &'static [CredentialField],
}

pub struct CredentialField {
    /// Clé du champ dans l'objet `credential`.
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub placeholder: &'static str,
    /// `text`, `password` ou `select`.
    pub input: &'static str,
    /// Valeurs proposées quand `input == "select"`, la première par défaut.
    pub choices: &'static [&'static str],
    pub required: bool,
}

/// Racine de la documentation en ligne, vers laquelle pointent les prérequis.
const DOCS_INSTALL: &str = "https://dumbmonit.selfhostr.io/docs/install/docker/";

/// Bloc Compose qui autorise l'utilisateur non root du conteneur à émettre des
/// échos ICMP. Identique à la documentation.
const ICMP_SNIPPET: &str = "services:
  dumbmonit:
    sysctls:
      net.ipv4.ping_group_range: \"0 2147483647\"
";

/// Ce que le déploiement doit fournir pour que `kind` fonctionne pleinement, et
/// si le serveur l'a constaté. Vide pour les types qui n'exigent rien.
///
/// Les textes affichés sont ceux de l'interface, par `id` : ici, seulement des
/// constats et des valeurs à copier.
fn requirements(kind: &str) -> Vec<Requirement> {
    match kind {
        "ping" => vec![Requirement {
            id: "icmp".into(),
            satisfied: dumbmonit_collectors::uptime::icmp_available(),
            doc_url: format!("{DOCS_INSTALL}#icmp-ping-without-a-capability").into(),
            compose_snippet: ICMP_SNIPPET.into(),
        }],
        _ => Vec::new(),
    }
}

/// Types encore en bêta : l'interface les signale par un badge (`beta` dans le
/// catalogue). Une seule liste : sortir un type de la bêta, c'est l'en retirer.
const BETA_KINDS: &[&str] = &[
    "arubacentral",
    "authentik",
    "ceph",
    "duplicati",
    "frigate",
    "keycloak",
    "kopia",
    "lidarr",
    "mailcow",
    "minio",
    "omada",
    "papra",
    "prowlarr",
    "qbittorrent",
    "qnap",
    "radarr",
    "sonarr",
];

pub async fn list(State(state): State<AppState>) -> Json<Vec<KindDescription>> {
    let registry = &state.collectors;
    let mut kinds = Vec::new();
    for kind in registry.kinds() {
        let mut description = describe(&kind, registry.get(&kind).and_then(|c| c.description()));
        // Mesures et vues : celles du collecteur ou du catalogue embarqué, sauf pour
        // un type né à l'exécution qui les a décrites lui-même.
        if description.metrics.is_empty()
            && description.views.is_empty()
            && let Some(measures) = registry.measures(&kind)
        {
            description.metrics = measures.metrics;
            description.views = measures.views;
        }
        // Les prérequis ne concernent que les types compilés : un paquet
        // d'intégration ne peut pas en déclarer sous le nom d'un type du serveur.
        if compiled(&kind).is_some() {
            description.requirements = requirements(&kind);
        }
        kinds.push(description);
    }
    Json(kinds)
}

/// Les clés d'options d'un type compilé, telles que les tables les décrivent.
///
/// Le registre les écarte des étiquettes de chaque série
/// (`Registry::base_labels`) : un port ou un certificat d'autorité sont des
/// réglages, pas des étiquettes que l'utilisateur a voulu poser.
pub fn option_keys(kind: &str) -> Vec<String> {
    compiled(kind)
        .map(|view| view.options.iter().map(|option| option.key.to_string()).collect())
        .unwrap_or_default()
}

/// Libellé d'affichage d'un type compilé ; son nom brut sinon.
pub fn kind_label(kind: &str) -> String {
    compiled(kind).map_or_else(|| kind.to_string(), |view| view.label.to_string())
}

/// Vrai si ce type compilé ne marche pas sans identifiants : il en déclare, et
/// aucune de ses formes n'est « none ». Un type inconnu ou sans identifiants :
/// faux (l'ajout en bloc le crée actif).
pub fn needs_credential(kind: &str) -> bool {
    compiled(kind)
        .is_some_and(|v| !v.credential_types.is_empty() && !v.credential_types.contains(&"none"))
}

/// Description d'un type : celle des tables de ce fichier s'il y figure, sinon
/// celle que fournit son collecteur (`Collector::description`, pour un type né à
/// l'exécution), à défaut une description générique.
///
/// Les tables ont le dernier mot : un collecteur ne peut pas changer en silence la
/// notice d'un type compilé.
fn describe(kind: &str, own: Option<KindDescription>) -> KindDescription {
    match (compiled(kind), own) {
        (Some(view), _) => view.description(),
        // Le type sous lequel le collecteur est enregistré fait foi : c'est lui
        // que les cibles portent.
        (None, Some(own)) => KindDescription { kind: kind.to_owned().into(), ..own },
        (None, None) => generic(kind),
    }
}

/// Les types compilés dans le serveur, décrits par les tables de ce module : une
/// fonction par famille (`kinds_*.rs`), puis les intégrations en nuage.
fn compiled(kind: &str) -> Option<CollectorView> {
    kinds_devices::view(kind)
        .or_else(|| kinds_probes::view(kind))
        .or_else(|| kinds_apps::view(kind))
        .or_else(|| kinds_services::view(kind))
        .or_else(|| cloud::view(kind))
}

/// Un type que ni les tables ni son collecteur ne décrivent : son nom brut, sans
/// notice, sans identifiant.
fn generic(kind: &str) -> KindDescription {
    KindDescription {
        kind: kind.to_owned().into(),
        label: kind.to_owned().into(),
        credential_types: vec![NO_AUTH.kind.into()],
        credentials: vec![NO_AUTH.description()],
        ..KindDescription::default()
    }
}

/// Page de la documentation du site (en anglais : l'interface choisit la
/// langue) qui détaille le type ; vide s'il n'en a pas.
fn guide_url(kind: &str) -> String {
    let page = match kind {
        "victorialogs" | "victoriametrics" => "victoriametrics",
        "agent" | "agent_service" => "agent",
        "agent_script" => "custom-scripts",
        "http_steps" => "http-scenario",
        "http" | "tcp" | "dns" | "ping" | "tls" | "ntp" | "smtp" | "mysql" | "mqtt"
        | "websocket" | "udp" | "ssh" | "imap" | "pop3" | "snmp_oid" | "radius" | "grpc" => {
            "services"
        }
        "postgres" => "services",
        "snmp" | "proxmox" | "pbs" | "pdm" | "pmg" | "synology" | "opnsense" | "truenas"
        | "redfish" | "loki" | "graylog" | "mdaemon" | "securitygateway" | "nextcloud"
        | "immich" | "sonarr" | "radarr" | "lidarr" | "prowlarr" | "qbittorrent" | "frigate"
        | "keycloak" | "authentik" | "vaultwarden" | "mailcow" | "paperless" | "papra"
        | "jellyfin" | "plex" | "gitlab" | "forgejo" | "pihole" | "adguard" | "nut"
        | "mikrotik" | "unifi" | "homeassistant" | "vsphere" | "redis" | "mongodb" | "rabbitmq"
        | "crowdsec" | "traefik" | "caddy" | "nginx" | "apache" | "npm" | "domain" | "provider"
        | "mail_blacklist" | "ip_blacklist" | "kubernetes" | "activedirectory" | "pfsense"
        | "unraid" | "veeam" | "winrm" | "tailscale" | "fortigate" | "sophos" | "omada"
        | "arubacentral" | "qnap" | "ceph" | "minio" | "kopia" | "duplicati" | "openvpn"
        | "xcpng" | "nutanix" | "mssql" | "oracle" | "elasticsearch" | "influxdb" | "kafka"
        | "push" | "webchange" | "website" | "netconfig" | "m365" | "entra" | "azure" | "aws"
        | "cloudflare" | "hetzner" | "ovh" | "infomaniak" | "prometheus" | "otlp" => kind,
        _ => return String::new(),
    };
    let anchor = match kind {
        "http" | "tcp" | "dns" | "ping" | "tls" | "ntp" | "smtp" | "mqtt" | "websocket" | "udp"
        | "ssh" | "imap" | "pop3" | "radius" | "grpc" => format!("#{kind}"),
        "postgres" => "#postgresql".to_owned(),
        "snmp_oid" => "#snmp-oid".to_owned(),
        "mysql" => "#mysql-and-mariadb".to_owned(),
        _ => String::new(),
    };
    format!("https://dumbmonit.selfhostr.io/docs/devices/{page}/{anchor}")
}

/// Les textes des tables, empruntés plutôt que copiés.
fn texts(values: &[&'static str]) -> Vec<Text> {
    values.iter().map(|&value| value.into()).collect()
}

impl CollectorView {
    /// La forme servie par l'API, commune aux types compilés et à ceux qui se
    /// décrivent eux-mêmes.
    fn description(&self) -> KindDescription {
        KindDescription {
            kind: self.kind.into(),
            label: self.label.into(),
            summary: self.summary.into(),
            examples: texts(self.examples),
            credential_types: texts(self.credential_types),
            credentials: self.credentials.iter().map(CredentialView::description).collect(),
            address_hint: self.address_hint.into(),
            default_port: self.default_port,
            setup: SetupDescription {
                title: self.setup.title.into(),
                steps: texts(self.setup.steps),
                ports: self.setup.ports.iter().map(PortView::description).collect(),
                permissions: texts(self.setup.permissions),
                warning: self.setup.warning.into(),
                doc_url: guide_url(self.kind).into(),
            },
            options: self.options.iter().map(OptionView::description).collect(),
            metrics: Vec::new(),
            views: Vec::new(),
            requirements: Vec::new(),
            beta: BETA_KINDS.contains(&self.kind),
        }
    }
}

impl OptionView {
    fn description(&self) -> OptionDescription {
        OptionDescription {
            key: self.key.into(),
            label: self.label.into(),
            help: self.help.into(),
            placeholder: self.placeholder.into(),
            default: self.default.into(),
            required: self.required,
            input: self.input.into(),
            choices: texts(self.choices),
            group: self.group.into(),
            items: (self.items)(),
            columns: (self.columns)(),
        }
    }
}

impl CredentialView {
    fn description(&self) -> CredentialDescription {
        CredentialDescription {
            kind: self.kind.into(),
            label: self.label.into(),
            help: self.help.into(),
            fields: self
                .fields
                .iter()
                .map(|field| CredentialFieldDescription {
                    key: field.key.into(),
                    label: field.label.into(),
                    help: field.help.into(),
                    placeholder: field.placeholder.into(),
                    input: field.input.into(),
                    choices: texts(field.choices),
                    required: field.required,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests;
