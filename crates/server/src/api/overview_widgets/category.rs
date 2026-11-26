//! Famille d'un moniteur d'après son type, et libellé court des sondes de
//! service. Une seule table : un type inconnu (paquet d'intégration, type né
//! à l'exécution) tombe dans « autre » plutôt que d'être deviné.

use super::model::Category;

/// Type de collecteur → famille. Ce qui n'y figure pas est [`Category::Other`].
const CATEGORIES: &[(&str, Category)] = &[
    // Serveurs, hyperviseurs, nuages.
    ("agent", Category::Servers),
    ("winrm", Category::Servers),
    ("proxmox", Category::Servers),
    ("pdm", Category::Servers),
    ("xcpng", Category::Servers),
    ("vsphere", Category::Servers),
    ("nutanix", Category::Servers),
    ("redfish", Category::Servers),
    ("kubernetes", Category::Servers),
    ("aws", Category::Servers),
    ("azure", Category::Servers),
    ("hetzner", Category::Servers),
    ("ovh", Category::Servers),
    ("infomaniak", Category::Servers),
    ("dummy", Category::Servers),
    ("nut", Category::Servers),
    // Stockage.
    ("synology", Category::Storage),
    ("truenas", Category::Storage),
    ("qnap", Category::Storage),
    ("unraid", Category::Storage),
    ("ceph", Category::Storage),
    ("minio", Category::Storage),
    // Réseau.
    ("snmp", Category::Network),
    ("snmp_oid", Category::Network),
    ("ping", Category::Network),
    ("unifi", Category::Network),
    ("omada", Category::Network),
    ("arubacentral", Category::Network),
    ("mikrotik", Category::Network),
    ("opnsense", Category::Network),
    ("pfsense", Category::Network),
    ("fortigate", Category::Network),
    ("sophos", Category::Network),
    ("openvpn", Category::Network),
    ("tailscale", Category::Network),
    ("cloudflare", Category::Network),
    ("adguard", Category::Network),
    ("pihole", Category::Network),
    ("ip_blacklist", Category::Network),
    // Sauvegardes.
    ("pbs", Category::Backups),
    ("veeam", Category::Backups),
    ("duplicati", Category::Backups),
    ("kopia", Category::Backups),
    ("netconfig", Category::Backups),
    // Services : sondes, bases, applications.
    ("http", Category::Services),
    ("http_steps", Category::Services),
    ("tcp", Category::Services),
    ("udp", Category::Services),
    ("dns", Category::Services),
    ("tls", Category::Services),
    ("ntp", Category::Services),
    ("smtp", Category::Services),
    ("imap", Category::Services),
    ("pop3", Category::Services),
    ("ssh", Category::Services),
    ("radius", Category::Services),
    ("grpc", Category::Services),
    ("websocket", Category::Services),
    ("mqtt", Category::Services),
    ("push", Category::Services),
    ("provider", Category::Services),
    ("domain", Category::Services),
    ("website", Category::Services),
    ("webchange", Category::Services),
    ("mail_blacklist", Category::Services),
    ("postgres", Category::Services),
    ("mysql", Category::Services),
    ("mssql", Category::Services),
    ("oracle", Category::Services),
    ("mongodb", Category::Services),
    ("redis", Category::Services),
    ("elasticsearch", Category::Services),
    ("influxdb", Category::Services),
    ("kafka", Category::Services),
    ("rabbitmq", Category::Services),
    ("victoriametrics", Category::Services),
    ("victorialogs", Category::Services),
    ("loki", Category::Services),
    ("graylog", Category::Services),
    ("prometheus", Category::Services),
    ("otlp", Category::Services),
    ("agent_service", Category::Services),
    ("activedirectory", Category::Services),
    ("entra", Category::Services),
    ("m365", Category::Services),
    ("keycloak", Category::Services),
    ("authentik", Category::Services),
    ("mailcow", Category::Services),
    ("mdaemon", Category::Services),
    ("securitygateway", Category::Services),
    ("pmg", Category::Services),
    ("nextcloud", Category::Services),
    ("immich", Category::Services),
    ("jellyfin", Category::Services),
    ("plex", Category::Services),
    ("homeassistant", Category::Services),
    ("paperless", Category::Services),
    ("papra", Category::Services),
    ("frigate", Category::Services),
    ("vaultwarden", Category::Services),
    ("gitlab", Category::Services),
    ("forgejo", Category::Services),
    ("nginx", Category::Services),
    ("npm", Category::Services),
    ("apache", Category::Services),
    ("caddy", Category::Services),
    ("traefik", Category::Services),
    ("crowdsec", Category::Services),
    ("qbittorrent", Category::Services),
    ("sonarr", Category::Services),
    ("radarr", Category::Services),
    ("lidarr", Category::Services),
    ("prowlarr", Category::Services),
];

pub fn category_of(kind: &str) -> Category {
    CATEGORIES.iter().find(|(known, _)| *known == kind).map_or(Category::Other, |(_, c)| *c)
}

/// Sondes listées dans le tableau des services (`services`).
pub const SERVICE_KINDS: &[&str] = &["http", "http_steps", "dns", "tls", "smtp", "ping", "tcp"];

/// Types dont l'état vient du résultat de la sonde (`dumbmonit_probe_success`)
/// plutôt que de la dernière interrogation : miroir de `UPTIME_KINDS` et
/// `PUSH_KIND` de l'interface (`web/src/lib/format.ts`).
const PROBE_STATE_KINDS: &[&str] = &[
    "http",
    "http_steps",
    "tcp",
    "dns",
    "ping",
    "tls",
    "ntp",
    "smtp",
    "postgres",
    "mysql",
    "mqtt",
    "websocket",
    "udp",
    "ssh",
    "imap",
    "pop3",
    "snmp_oid",
    "radius",
    "grpc",
    "provider",
    "push",
];

pub fn has_probe_state(kind: &str) -> bool {
    PROBE_STATE_KINDS.contains(&kind)
}

/// Libellé court d'une sonde de service (« HTTPS », « DNS »…), `None` pour
/// les autres types. Une sonde HTTP vers une adresse `https://` est dite HTTPS.
pub fn service_label(kind: &str, address: &str) -> Option<&'static str> {
    Some(match kind {
        "http" | "http_steps" => {
            if address.trim().to_ascii_lowercase().starts_with("https://") {
                "HTTPS"
            } else {
                "HTTP"
            }
        }
        "dns" => "DNS",
        "tls" => "TLS",
        "smtp" => "SMTP",
        "ping" => "Ping",
        "tcp" => "TCP",
        "udp" => "UDP",
        "ntp" => "NTP",
        "ssh" => "SSH",
        "imap" => "IMAP",
        "pop3" => "POP3",
        "grpc" => "gRPC",
        "websocket" => "WebSocket",
        "push" => "Heartbeat",
        _ => return None,
    })
}

/// Libellé court du type d'un moniteur : « HTTPS check » pour une sonde, le
/// nom du catalogue pour le reste.
pub fn kind_label(kind: &str, address: &str) -> String {
    match service_label(kind, address) {
        Some(short) => format!("{short} check"),
        None => crate::api::collectors::kind_label(kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaque_type_tombe_dans_sa_famille() {
        assert_eq!(category_of("proxmox"), Category::Servers);
        assert_eq!(category_of("agent"), Category::Servers);
        assert_eq!(category_of("synology"), Category::Storage);
        assert_eq!(category_of("truenas"), Category::Storage);
        assert_eq!(category_of("snmp"), Category::Network);
        assert_eq!(category_of("unifi"), Category::Network);
        assert_eq!(category_of("http"), Category::Services);
        assert_eq!(category_of("postgres"), Category::Services);
        assert_eq!(category_of("pbs"), Category::Backups);
        assert_eq!(category_of("veeam"), Category::Backups);
        // Un type inconnu n'est pas deviné.
        assert_eq!(category_of("zz-inconnu"), Category::Other);
        assert_eq!(category_of(""), Category::Other);
    }

    #[test]
    fn la_table_ne_cite_chaque_type_qu_une_fois() {
        let mut kinds: Vec<&str> = CATEGORIES.iter().map(|(kind, _)| *kind).collect();
        let before = kinds.len();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), before, "type en double dans CATEGORIES");
    }

    #[test]
    fn toute_sonde_de_service_est_un_service() {
        for kind in SERVICE_KINDS {
            assert!(has_probe_state(kind), "{kind}");
            assert!(service_label(kind, "").is_some(), "{kind}");
        }
        assert_eq!(category_of("ping"), Category::Network);
        for kind in SERVICE_KINDS.iter().filter(|k| **k != "ping") {
            assert_eq!(category_of(kind), Category::Services, "{kind}");
        }
    }

    #[test]
    fn une_sonde_http_vers_https_est_dite_https() {
        assert_eq!(service_label("http", "https://example.org"), Some("HTTPS"));
        assert_eq!(service_label("http", "http://example.org"), Some("HTTP"));
        assert_eq!(kind_label("dns", "example.org"), "DNS check");
        assert_eq!(service_label("proxmox", "10.0.0.1"), None);
    }
}
