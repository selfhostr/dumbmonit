use super::*;

/// La bêta est une donnée du catalogue, pas du libellé : chaque type listé existe,
/// et son libellé ne porte plus « (beta) ».
#[test]
fn la_beta_est_un_champ_et_non_un_suffixe_du_libelle() {
    for kind in BETA_KINDS {
        let view = describe(kind);
        assert!(view.description().beta, "{kind} devrait être en bêta");
        assert!(!view.label.contains("(beta)"), "{kind} : « (beta) » reste dans le libellé");
    }
    assert!(!describe("snmp").description().beta);
}

/// Un type compilé, tel que les tables le décrivent.
fn describe(kind: &str) -> CollectorView {
    compiled(kind).unwrap_or_else(|| panic!("« {kind} » absent des tables"))
}

/// Les types enregistrés dans `main.rs`. Un type ajouté là-bas sans notice ici
/// s'afficherait sous son nom brut, sans explication ni exemple d'adresse.
const KINDS_ENREGISTRES: &[&str] = &[
    "snmp",
    "proxmox",
    "pbs",
    "pdm",
    "pmg",
    "synology",
    "opnsense",
    "truenas",
    "redfish",
    "victoriametrics",
    "victorialogs",
    "loki",
    "graylog",
    "mdaemon",
    "securitygateway",
    "nextcloud",
    "immich",
    "paperless",
    "papra",
    "jellyfin",
    "plex",
    "gitlab",
    "forgejo",
    "sonarr",
    "radarr",
    "lidarr",
    "prowlarr",
    "qbittorrent",
    "frigate",
    "keycloak",
    "authentik",
    "vaultwarden",
    "mailcow",
    "pihole",
    "adguard",
    "nut",
    "mikrotik",
    "unifi",
    "homeassistant",
    "vsphere",
    "redis",
    "mongodb",
    "rabbitmq",
    "mssql",
    "oracle",
    "elasticsearch",
    "influxdb",
    "kafka",
    "crowdsec",
    "traefik",
    "caddy",
    "nginx",
    "apache",
    "npm",
    "domain",
    "mail_blacklist",
    "ip_blacklist",
    "kubernetes",
    "activedirectory",
    "pfsense",
    "unraid",
    "veeam",
    "winrm",
    "kopia",
    "duplicati",
    "tailscale",
    "m365",
    "entra",
    "azure",
    "aws",
    "gcp",
    "ipmi",
    "cloudflare",
    "hetzner",
    "ovh",
    "infomaniak",
    "fortigate",
    "arubacentral",
    "sophos",
    "omada",
    "qnap",
    "ceph",
    "minio",
    "openvpn",
    "xcpng",
    "nutanix",
    "agent",
    "agent_service",
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
    "provider",
    "push",
    "webchange",
    "website",
    "netconfig",
    "dummy",
    "snmp_oid",
    "radius",
    "grpc",
];

#[test]
fn chaque_type_enregistre_a_une_notice_qui_lui_est_propre() {
    for kind in KINDS_ENREGISTRES {
        let view = describe(kind);
        assert_eq!(view.kind, *kind);
        assert!(
            view.label != *kind || !view.summary.is_empty(),
            "« {kind} » n'a que la description générique"
        );
        assert!(!view.setup.title.is_empty(), "« {kind} » n'a pas de notice de mise en route");
        assert!(!view.setup.steps.is_empty(), "« {kind} » n'a aucune étape");
        assert!(!view.address_hint.is_empty(), "« {kind} » n'a pas d'adresse d'exemple");
        assert!(
            !view.credential_types.is_empty(),
            "« {kind} » n'annonce aucune forme d'identifiant"
        );
    }
}

#[test]
fn un_type_inconnu_recoit_une_description_generique_mais_complete() {
    let view = super::describe("inconnu", None);
    assert_eq!(view.kind, "inconnu");
    assert_eq!(view.label, "inconnu");
    assert!(view.summary.is_empty());
    assert!(view.options.is_empty());
    assert_eq!(view.credential_types, ["none"]);
    assert_eq!(view.credentials, [NO_AUTH.description()]);
}

/// Un type né à l'exécution se décrit lui-même, sous le type où il est
/// enregistré ; un type compilé garde la notice de ses tables.
#[test]
fn un_collecteur_absent_des_tables_se_decrit_lui_meme() {
    let own = KindDescription {
        kind: "autre-nom".into(),
        label: "Imprimante du paquet".into(),
        summary: "Décrite par son paquet.".into(),
        ..KindDescription::default()
    };
    let view = super::describe("pack_printer", Some(own.clone()));
    assert_eq!(view.kind, "pack_printer");
    assert_eq!(view.label, "Imprimante du paquet");
    assert_eq!(view.summary, "Décrite par son paquet.");

    let view = super::describe("snmp", Some(own));
    assert_eq!(view.label, "SNMP device");
    assert_eq!(view, compiled("snmp").unwrap().description());
}

/// Les sondes lisent ces étiquettes dans leurs `options.rs` : l'interface ne
/// doit pas proposer un champ que le collecteur ignorerait, ni en cacher un.
#[test]
fn les_options_des_sondes_reprennent_les_etiquettes_lues_par_les_collecteurs() {
    let attendues: &[(&str, &[&str])] = &[
        (
            "http",
            &[
                "method",
                "accepted_status",
                "keyword",
                "keyword_absent",
                "keyword_case_sensitive",
                "json_path",
                "json_expect",
                "headers",
                "body",
                "follow_redirects",
                "max_redirects",
                "insecure_tls",
                "allow_private_targets",
                "check_certificate",
                "max_body_bytes",
                "user_agent",
                "timeout_seconds",
            ],
        ),
        (
            "http_steps",
            &[
                "steps",
                "follow_redirects",
                "max_redirects",
                "insecure_tls",
                "allow_private_targets",
                "check_certificate",
                "max_body_bytes",
                "user_agent",
                "timeout_seconds",
            ],
        ),
        ("tcp", &["port", "allow_private_targets", "timeout_seconds"]),
        (
            "dns",
            &[
                "record_type",
                "resolver",
                "expect",
                "expect_mode",
                "forbid",
                "alert_on_change",
                "timeout_seconds",
            ],
        ),
        (
            "ping",
            &[
                "count",
                "packet_timeout_ms",
                "interval_ms",
                "payload_bytes",
                "ip_version",
                "loss_threshold_percent",
                "timeout_seconds",
            ],
        ),
        ("tls", &["server_name", "insecure_tls", "allow_private_targets", "timeout_seconds"]),
        ("ntp", &["port", "offset_threshold_ms", "allow_private_targets", "timeout_seconds"]),
        (
            "smtp",
            &[
                "security",
                "port",
                "helo_name",
                "expect_capability",
                "server_name",
                "insecure_tls",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "postgres",
            &[
                "port",
                "database",
                "query",
                "expect",
                "sslmode",
                "server_stats",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "mysql",
            &[
                "port",
                "database",
                "query",
                "expect",
                "sslmode",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "mqtt",
            &[
                "tls",
                "port",
                "topic",
                "expect_message",
                "expect",
                "client_id",
                "server_name",
                "insecure_tls",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "websocket",
            &[
                "path",
                "port",
                "send",
                "expect",
                "subprotocol",
                "origin",
                "server_name",
                "insecure_tls",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "udp",
            &[
                "port",
                "payload",
                "payload_encoding",
                "expect_response",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "ssh",
            &[
                "port",
                "expected_host_key",
                "alert_on_change",
                "host_key_type",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "imap",
            &[
                "security",
                "port",
                "server_name",
                "insecure_tls",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "pop3",
            &[
                "security",
                "port",
                "server_name",
                "insecure_tls",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "snmp_oid",
            &[
                "oid",
                "port",
                "min",
                "max",
                "expect",
                "snmp_version",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        ("radius", &["port", "nas_identifier", "allow_private_targets", "timeout_seconds"]),
        (
            "grpc",
            &[
                "service",
                "tls",
                "port",
                "server_name",
                "insecure_tls",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "proxmox",
            &[
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "backup_lookback_days",
                "scan_backup_storage",
                "nodes",
                "ha",
                "backup_jobs",
                "scan_snapshots",
                "max_snapshot_guests",
                "replication",
                "ceph",
                "updates",
                "certificates",
                "guest_agent",
                "disks",
                "zfs",
                "packages",
                "subscription",
                "cluster_resources",
                "services",
                "network",
                "lvm",
                "ceph_detail",
                "backup_volumes",
                "guest_os",
                "metrics_export",
            ],
        ),
        (
            "pbs",
            &[
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "task_lookback_hours",
                "datastores",
                "max_groups",
                "jobs",
                "updates",
                "disks",
                "services",
                "datastore_details",
                "traffic_control",
                "certificates",
                "tape",
                "device_stale_days",
            ],
        ),
        (
            "pdm",
            &[
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "node",
                "task_lookback_hours",
                "max_age_seconds",
                "remotes",
                "max_remotes",
                "versions",
                "tasks",
                "node_status",
                "updates",
                "remote_updates",
            ],
        ),
        (
            "pmg",
            &[
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "node",
                "recent_hours",
                "queues",
                "quarantine",
                "attachment_quarantine",
                "signatures",
                "services",
                "certificates",
                "updates",
                "subscription",
            ],
        ),
        (
            "synology",
            &[
                "scheme",
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "abb",
                "drive",
                "photos",
            ],
        ),
        (
            "truenas",
            &[
                "scheme",
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "datasets",
                "disks",
                "smart",
                "alerts",
                "tasks",
                "services",
            ],
        ),
        (
            "opnsense",
            &[
                "scheme",
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "gateways",
                "interfaces",
                "firewall",
                "dhcp",
                "vpn",
                "unbound",
                "services",
                "carp",
                "firmware",
                "temperature",
                "certificates",
                "live_map",
            ],
        ),
        ("push", &["expected_interval", "grace", "schedule", "timezone", "max_duration"]),
        (
            "webchange",
            &[
                "scope",
                "max_pages",
                "path_prefix",
                "ignore",
                "content_selector",
                "exclude_selector",
                "keep_page_chrome",
                "ignore_query",
                "user_agent",
                "check_interval_minutes",
                "insecure_tls",
                "allow_private_targets",
            ],
        ),
        (
            "netconfig",
            &[
                "profile",
                "fetch_interval_hours",
                "keep_versions",
                "keep_days",
                "fetch_timeout_seconds",
                "insecure_tls",
            ],
        ),
        (
            "website",
            &[
                "expect_text",
                "min_text_chars",
                "max_redirects",
                "category",
                "user_agent",
                "ignore_query",
                "change_check_minutes",
                "confirm_checks",
                "text_drop_percent",
                "watch_final_url",
                "watch_title",
                "watch_description",
                "watch_canonical",
                "watch_robots",
                "watch_open_graph",
                "watch_lang",
                "watch_h1",
                "watch_hreflang",
                "watch_google_verification",
                "host_spacing_seconds",
                "allow_private_targets",
            ],
        ),
        (
            "redfish",
            &["port", "insecure_tls", "request_timeout_seconds", "auth", "storage", "logs"],
        ),
        ("victoriametrics", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("victorialogs", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("loki", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("graylog", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("pihole", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        (
            "adguard",
            &["scheme", "port", "insecure_tls", "request_timeout_seconds", "upstream_check"],
        ),
        ("nut", &["port", "ups", "request_timeout_seconds"]),
        ("openvpn", &["port", "request_timeout_seconds"]),
        (
            "mikrotik",
            &[
                "scheme",
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "interfaces",
                "max_interfaces",
            ],
        ),
        (
            "mdaemon",
            &[
                "agent_id",
                "services",
                "request_timeout_seconds",
                "api_port",
                "api_tls",
                "insecure_tls",
            ],
        ),
        (
            "securitygateway",
            &[
                "agent_id",
                "services",
                "request_timeout_seconds",
                "api_port",
                "api_tls",
                "insecure_tls",
                "counters",
            ],
        ),
        ("nextcloud", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        (
            "immich",
            &["scheme", "port", "insecure_tls", "request_timeout_seconds", "device_stale_days"],
        ),
        (
            "paperless",
            &["scheme", "port", "insecure_tls", "request_timeout_seconds", "task_lookback_hours"],
        ),
        ("papra", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("jellyfin", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("plex", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        (
            "gitlab",
            &[
                "scheme",
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "watched_projects",
                "pipeline_lookback_hours",
            ],
        ),
        ("forgejo", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("sonarr", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("radarr", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("lidarr", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("prowlarr", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("qbittorrent", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("frigate", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        (
            "keycloak",
            &[
                "scheme",
                "port",
                "health_port",
                "health_scheme",
                "auth_realm",
                "realms",
                "insecure_tls",
                "request_timeout_seconds",
            ],
        ),
        ("authentik", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("vaultwarden", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("mailcow", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        (
            "unifi",
            &[
                "site",
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "watched_clients",
                "device_stale_days",
            ],
        ),
        (
            "homeassistant",
            &[
                "scheme",
                "port",
                "insecure_tls",
                "request_timeout_seconds",
                "battery_threshold",
                "exclude_domains",
                "repairs",
                "device_stale_days",
            ],
        ),
        ("vsphere", &["port", "insecure_tls", "request_timeout_seconds", "alarms"]),
        ("redis", &["port", "tls", "insecure_tls", "request_timeout_seconds"]),
        ("mongodb", &["port", "auth_source", "tls", "insecure_tls", "request_timeout_seconds"]),
        ("rabbitmq", &["scheme", "port", "insecure_tls", "request_timeout_seconds", "max_queues"]),
        ("mssql", &["port", "database", "tls", "insecure_tls", "request_timeout_seconds"]),
        (
            "oracle",
            &["scheme", "port", "ords_path", "schema", "insecure_tls", "request_timeout_seconds"],
        ),
        ("elasticsearch", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("influxdb", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("kafka", &["port", "tls", "insecure_tls", "request_timeout_seconds", "max_groups"]),
        (
            "crowdsec",
            &["scheme", "port", "lapi", "lapi_port", "insecure_tls", "request_timeout_seconds"],
        ),
        ("traefik", &["scheme", "port", "metrics", "insecure_tls", "request_timeout_seconds"]),
        ("caddy", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        (
            "nginx",
            &[
                "scheme",
                "port",
                "status_path",
                "insecure_tls",
                "request_timeout_seconds",
                "plus_api",
                "plus_api_version",
            ],
        ),
        ("apache", &["scheme", "port", "status_path", "insecure_tls", "request_timeout_seconds"]),
        (
            "npm",
            &[
                "scheme",
                "port",
                "certificates",
                "tls_port",
                "insecure_tls",
                "request_timeout_seconds",
            ],
        ),
        ("domain", &["rdap_server", "refresh_hours", "request_timeout_seconds"]),
        (
            "provider",
            &[
                "component",
                "fail_on",
                "refresh_seconds",
                "window_hours",
                "allow_private_targets",
                "timeout_seconds",
            ],
        ),
        (
            "mail_blacklist",
            &[
                "resolver",
                "lists",
                "custom_lists",
                "extra_addresses",
                "refresh_hours",
                "queries_per_second",
                "query_timeout_seconds",
            ],
        ),
        (
            "ip_blacklist",
            &[
                "resolver",
                "lists",
                "custom_lists",
                "refresh_hours",
                "queries_per_second",
                "query_timeout_seconds",
            ],
        ),
        (
            "kubernetes",
            &["port", "ca_cert", "insecure_tls", "request_timeout_seconds", "exclude_namespaces"],
        ),
        ("pfsense", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("unraid", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        (
            "veeam",
            &[
                "scheme",
                "port",
                "api_version",
                "insecure_tls",
                "request_timeout_seconds",
                "device_stale_days",
            ],
        ),
        (
            "winrm",
            &[
                "allow_unencrypted",
                "port",
                "auth",
                "insecure_tls",
                "request_timeout_seconds",
                "services",
                "ignore_services",
                "event_logs",
                "event_minutes",
            ],
        ),
        ("kopia", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("duplicati", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("tailscale", &["tailnet", "watch", "request_timeout_seconds", "device_stale_days"]),
        ("fortigate", &["port", "vdom", "insecure_tls", "request_timeout_seconds"]),
        ("arubacentral", &["token_address", "request_timeout_seconds"]),
        ("sophos", &["port", "insecure_tls", "request_timeout_seconds"]),
        ("omada", &["port", "omadac_id", "site", "insecure_tls", "request_timeout_seconds"]),
        ("qnap", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("ceph", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("minio", &["scheme", "port", "insecure_tls", "request_timeout_seconds"]),
        ("xcpng", &["port", "insecure_tls", "request_timeout_seconds"]),
        ("nutanix", &["port", "insecure_tls", "request_timeout_seconds"]),
    ];
    for (kind, cles) in attendues {
        let obtenues: Vec<&str> = describe(kind).options.iter().map(|o| o.key).collect();
        assert_eq!(&obtenues, cles, "options de « {kind} »");
    }
    for kind in ["snmp", "agent", "agent_service", "dummy"] {
        assert!(describe(kind).options.is_empty(), "« {kind} » ne lit aucune étiquette");
    }
}

#[test]
fn chaque_option_est_coherente_avec_son_type_de_champ() {
    for kind in KINDS_ENREGISTRES {
        for option in describe(kind).options {
            let contexte = format!("option « {} » de « {kind} »", option.key);
            assert!(!option.label.is_empty(), "{contexte} : sans libellé");
            assert!(!option.help.is_empty(), "{contexte} : sans aide");
            match option.input {
                "select" => {
                    assert!(!option.choices.is_empty(), "{contexte} : liste sans choix");
                    assert!(
                        option.choices.contains(&option.default),
                        "{contexte} : le défaut « {} » n'est pas dans la liste",
                        option.default
                    );
                }
                "boolean" => assert!(
                    matches!(option.default, "true" | "false"),
                    "{contexte} : défaut booléen « {} »",
                    option.default
                ),
                "number" => assert!(
                    option.default.is_empty() || option.default.parse::<u64>().is_ok(),
                    "{contexte} : défaut numérique « {} »",
                    option.default
                ),
                "text" => assert!(option.choices.is_empty(), "{contexte} : choix sur un texte"),
                // Le choix d'un agent : l'interface liste les agents connus, aucun défaut.
                "agent" => assert!(option.default.is_empty(), "{contexte} : défaut sur un agent"),
                "checklist" => {
                    let items = (option.items)();
                    assert!(!items.is_empty(), "{contexte} : cases à cocher sans entrée");
                    for value in option.default.split(',').filter(|v| !v.is_empty()) {
                        assert!(
                            items.iter().any(|i| i.value == value),
                            "{contexte} : le défaut « {value} » n'est pas une case"
                        );
                    }
                }
                "rows" => {
                    assert!(!(option.columns)().is_empty(), "{contexte} : lignes sans colonne")
                }
                // L'éditeur d'étapes d'un scénario HTTP : un tableau JSON, sans défaut.
                "steps" => assert!(option.default.is_empty(), "{contexte} : défaut sur des étapes"),
                other => panic!("{contexte} : type de champ inconnu « {other} »"),
            }
        }
    }
}

/// Les défauts affichés doivent être ceux que les collecteurs appliquent
/// réellement : un défaut faux dans le formulaire est pire qu'aucun.
#[test]
fn les_defauts_affiches_sont_ceux_des_collecteurs() {
    let defaut = |kind: &'static str, cle: &str| {
        describe(kind).options.iter().find(|o| o.key == cle).map(|o| o.default).unwrap()
    };
    assert_eq!(defaut("http", "timeout_seconds"), "5");
    assert_eq!(defaut("http", "max_body_bytes"), "524288");
    assert_eq!(defaut("http", "max_redirects"), "10");
    assert_eq!(defaut("http", "user_agent"), format!("DumbMonit/{}", env!("CARGO_PKG_VERSION")));
    assert_eq!(defaut("ping", "count"), "4");
    assert_eq!(defaut("ping", "packet_timeout_ms"), "1000");
    assert_eq!(defaut("proxmox", "port"), "8006");
    assert_eq!(defaut("proxmox", "backup_lookback_days"), "31");
    assert_eq!(defaut("pbs", "port"), "8007");
    assert_eq!(defaut("pbs", "request_timeout_seconds"), "15");
    assert_eq!(defaut("pbs", "task_lookback_hours"), "24");
    assert_eq!(defaut("pbs", "max_groups"), "500");
    assert_eq!(defaut("pdm", "port"), "8443");
    assert_eq!(defaut("pdm", "request_timeout_seconds"), "20");
    assert_eq!(defaut("pdm", "node"), "localhost");
    assert_eq!(defaut("pdm", "max_age_seconds"), "60");
    assert_eq!(defaut("pdm", "max_remotes"), "100");
    assert_eq!(defaut("pdm", "remote_updates"), "false");
    assert_eq!(defaut("pmg", "port"), "8006");
    assert_eq!(defaut("pmg", "request_timeout_seconds"), "15");
    assert_eq!(defaut("pmg", "recent_hours"), "12");
    assert_eq!(defaut("pmg", "attachment_quarantine"), "false");
    assert_eq!(defaut("synology", "request_timeout_seconds"), "15");
    assert_eq!(defaut("synology", "abb"), "true");
    assert_eq!(defaut("activedirectory", "security"), "ldaps");
    assert_eq!(defaut("activedirectory", "check_all_dcs"), "true");
    assert_eq!(defaut("activedirectory", "allow_plaintext"), "false");
    assert_eq!(
        defaut("activedirectory", "stale_days_users"),
        dumbmonit_collectors::activedirectory::DEFAULT_STALE_DAYS.to_string()
    );
    assert_eq!(
        defaut("activedirectory", "inventory_minutes"),
        dumbmonit_collectors::activedirectory::DEFAULT_INVENTORY_MINUTES.to_string()
    );
    assert_eq!(
        defaut("activedirectory", "request_timeout_seconds"),
        dumbmonit_collectors::activedirectory::DEFAULT_REQUEST_TIMEOUT_SECONDS.to_string()
    );
    assert_eq!(defaut("synology", "drive"), "true");
    assert_eq!(defaut("synology", "photos"), "true");
    assert_eq!(defaut("redfish", "port"), "443");
    assert_eq!(defaut("redfish", "request_timeout_seconds"), "8");
    assert_eq!(defaut("redfish", "auth"), "basic");
    // `collectors/observability/{options,victoria,loki,graylog}.rs`.
    assert_eq!(defaut("victoriametrics", "port"), "8428");
    assert_eq!(defaut("victorialogs", "port"), "9428");
    assert_eq!(defaut("loki", "port"), "3100");
    assert_eq!(defaut("graylog", "port"), "9000");
    for kind in ["victoriametrics", "victorialogs", "loki", "graylog"] {
        assert_eq!(defaut(kind, "scheme"), "http", "« {kind} »");
        assert_eq!(
            defaut(kind, "request_timeout_seconds"),
            dumbmonit_collectors::observability::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
        );
    }
    // `collectors/selfhosted/{options,nextcloud,immich,paperless,jellyfin,plex,gitlab,forgejo}.rs`.
    use dumbmonit_collectors::selfhosted;
    for (kind, scheme, port) in [
        ("nextcloud", "https", selfhosted::nextcloud::DEFAULT_PORT),
        ("immich", "http", selfhosted::immich::DEFAULT_PORT),
        ("paperless", "http", selfhosted::paperless::DEFAULT_PORT),
        ("papra", "http", selfhosted::papra::DEFAULT_PORT),
        ("jellyfin", "http", selfhosted::jellyfin::DEFAULT_PORT),
        ("plex", "http", selfhosted::plex::DEFAULT_PORT),
        ("gitlab", "https", selfhosted::gitlab::DEFAULT_PORT),
        ("forgejo", "http", selfhosted::forgejo::DEFAULT_PORT),
        ("sonarr", "http", selfhosted::arr::SONARR.port),
        ("radarr", "http", selfhosted::arr::RADARR.port),
        ("lidarr", "http", selfhosted::arr::LIDARR.port),
        ("prowlarr", "http", selfhosted::arr::PROWLARR.port),
        ("qbittorrent", "http", selfhosted::qbittorrent::DEFAULT_PORT),
        ("frigate", "http", selfhosted::frigate::DEFAULT_PORT),
        ("keycloak", "http", selfhosted::keycloak::DEFAULT_PORT),
        ("authentik", "http", selfhosted::authentik::DEFAULT_PORT),
        ("vaultwarden", "https", selfhosted::vaultwarden::DEFAULT_PORT),
        ("mailcow", "https", selfhosted::mailcow::DEFAULT_PORT),
    ] {
        assert_eq!(defaut(kind, "scheme"), scheme, "« {kind} »");
        assert_eq!(defaut(kind, "port"), port.to_string(), "« {kind} »");
        assert_eq!(describe(kind).default_port, port, "« {kind} »");
        assert_eq!(
            defaut(kind, "request_timeout_seconds"),
            selfhosted::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
        );
    }
    // `collectors/pihole/options.rs`.
    assert_eq!(defaut("pihole", "scheme"), "http");
    // `collectors/{unifi,homeassistant,vsphere}/mod.rs`.
    use dumbmonit_collectors::{homeassistant, unifi, vsphere};
    assert_eq!(defaut("unifi", "site"), unifi::DEFAULT_SITE);
    assert_eq!(defaut("unifi", "port"), unifi::DEFAULT_PORT.to_string());
    // `collectors/{socket,redis,mongodb,rabbitmq,crowdsec}`.
    assert_eq!(defaut("redis", "port"), dumbmonit_collectors::redis::DEFAULT_PORT.to_string());
    assert_eq!(defaut("mongodb", "port"), dumbmonit_collectors::mongodb::DEFAULT_PORT.to_string());
    assert_eq!(
        defaut("mongodb", "auth_source"),
        dumbmonit_collectors::mongodb::DEFAULT_AUTH_SOURCE
    );
    assert_eq!(
        defaut("rabbitmq", "port"),
        dumbmonit_collectors::rabbitmq::DEFAULT_PORT.to_string()
    );
    assert_eq!(
        defaut("rabbitmq", "max_queues"),
        dumbmonit_collectors::rabbitmq::DEFAULT_MAX_QUEUES.to_string()
    );
    // `collectors/{mssql,oracle,elasticsearch,influxdb,kafka}`.
    assert_eq!(defaut("mssql", "port"), dumbmonit_collectors::mssql::DEFAULT_PORT.to_string());
    assert_eq!(defaut("mssql", "database"), dumbmonit_collectors::mssql::DEFAULT_DATABASE);
    assert_eq!(defaut("oracle", "port"), dumbmonit_collectors::oracle::DEFAULT_PORT.to_string());
    assert_eq!(defaut("oracle", "ords_path"), dumbmonit_collectors::oracle::DEFAULT_ORDS_PATH);
    assert_eq!(
        defaut("elasticsearch", "port"),
        dumbmonit_collectors::elasticsearch::DEFAULT_PORT.to_string()
    );
    assert_eq!(
        defaut("influxdb", "port"),
        dumbmonit_collectors::influxdb::DEFAULT_PORT.to_string()
    );
    assert_eq!(defaut("kafka", "port"), dumbmonit_collectors::kafka::DEFAULT_PORT.to_string());
    assert_eq!(
        defaut("kafka", "max_groups"),
        dumbmonit_collectors::kafka::DEFAULT_MAX_GROUPS.to_string()
    );
    assert_eq!(
        defaut("crowdsec", "port"),
        dumbmonit_collectors::crowdsec::DEFAULT_PORT.to_string()
    );
    assert_eq!(
        defaut("crowdsec", "lapi_port"),
        dumbmonit_collectors::crowdsec::DEFAULT_LAPI_PORT.to_string()
    );
    assert_eq!(
        defaut("kubernetes", "port"),
        dumbmonit_collectors::kubernetes::DEFAULT_PORT.to_string()
    );
    assert_eq!(
        defaut("kubernetes", "request_timeout_seconds"),
        dumbmonit_collectors::kubernetes::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
    );
    // Équipements lus par leur API (`collectors/{pfsense,unraid,veeam,
    // tailscale,fortigate,sophos}`) : ports et délai de `rest.rs`.
    for (kind, port) in [
        ("pfsense", dumbmonit_collectors::pfsense::DEFAULT_PORT),
        ("unraid", dumbmonit_collectors::unraid::DEFAULT_PORT),
        ("veeam", dumbmonit_collectors::veeam::DEFAULT_PORT),
        ("kopia", dumbmonit_collectors::kopia::DEFAULT_PORT),
        ("duplicati", dumbmonit_collectors::duplicati::DEFAULT_PORT),
        ("fortigate", dumbmonit_collectors::fortigate::DEFAULT_PORT),
        ("sophos", dumbmonit_collectors::sophos::DEFAULT_PORT),
        ("omada", dumbmonit_collectors::omada::DEFAULT_PORT),
        ("qnap", dumbmonit_collectors::qnap::DEFAULT_PORT),
        ("ceph", dumbmonit_collectors::ceph::DEFAULT_PORT),
        ("minio", dumbmonit_collectors::minio::DEFAULT_PORT),
        ("xcpng", dumbmonit_collectors::xcpng::DEFAULT_PORT),
        ("nutanix", dumbmonit_collectors::nutanix::DEFAULT_PORT),
    ] {
        assert_eq!(defaut(kind, "port"), port.to_string(), "{kind}");
        assert_eq!(describe(kind).default_port, port, "{kind}");
    }
    for kind in [
        "pfsense",
        "unraid",
        "veeam",
        "tailscale",
        "fortigate",
        "sophos",
        "omada",
        "arubacentral",
        "qnap",
        "ceph",
        "minio",
        "kopia",
        "duplicati",
        "xcpng",
        "nutanix",
    ] {
        assert_eq!(
            defaut(kind, "request_timeout_seconds"),
            dumbmonit_collectors::selfhosted::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string(),
            "{kind}"
        );
    }
    assert_eq!(defaut("veeam", "api_version"), dumbmonit_collectors::veeam::DEFAULT_API_VERSION);
    assert_eq!(
        describe("tailscale").address_hint,
        dumbmonit_collectors::tailscale::DEFAULT_ADDRESS
    );
    assert_eq!(
        describe("arubacentral").address_hint,
        dumbmonit_collectors::arubacentral::DEFAULT_ADDRESS
    );
    assert_eq!(
        describe("arubacentral").default_port,
        dumbmonit_collectors::arubacentral::DEFAULT_PORT
    );
    assert_eq!(
        defaut("arubacentral", "token_address"),
        dumbmonit_collectors::arubacentral::DEFAULT_TOKEN_ADDRESS
    );
    for kind in ["redis", "mongodb"] {
        assert_eq!(
            defaut(kind, "request_timeout_seconds"),
            dumbmonit_collectors::SOCKET_DEFAULT_TIMEOUT.as_secs().to_string()
        );
        assert_eq!(defaut(kind, "tls"), "false");
    }
    assert_eq!(
        defaut("unifi", "request_timeout_seconds"),
        unifi::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
    );
    assert_eq!(defaut("homeassistant", "port"), homeassistant::DEFAULT_PORT.to_string());
    assert_eq!(defaut("homeassistant", "scheme"), "http");
    assert_eq!(
        defaut("homeassistant", "request_timeout_seconds"),
        homeassistant::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
    );
    assert_eq!(
        defaut("homeassistant", "battery_threshold"),
        homeassistant::DEFAULT_BATTERY_THRESHOLD.to_string()
    );
    assert_eq!(defaut("homeassistant", "repairs"), "true");
    assert_eq!(defaut("vsphere", "port"), vsphere::DEFAULT_PORT.to_string());
    assert_eq!(
        defaut("vsphere", "request_timeout_seconds"),
        vsphere::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
    );
    assert_eq!(defaut("vsphere", "alarms"), "true");
    assert_eq!(defaut("pihole", "port"), dumbmonit_collectors::pihole::DEFAULT_PORT.to_string());
    assert_eq!(
        defaut("pihole", "request_timeout_seconds"),
        dumbmonit_collectors::pihole::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
    );
    // `collectors/adguard/options.rs`.
    assert_eq!(defaut("adguard", "scheme"), "http");
    assert_eq!(defaut("adguard", "port"), dumbmonit_collectors::adguard::DEFAULT_PORT.to_string());
    assert_eq!(
        defaut("adguard", "request_timeout_seconds"),
        dumbmonit_collectors::adguard::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
    );
    assert_eq!(defaut("adguard", "upstream_check"), "true");
    assert_eq!(describe("adguard").default_port, dumbmonit_collectors::adguard::DEFAULT_PORT);
    // `collectors/nut/mod.rs`.
    assert_eq!(defaut("nut", "port"), dumbmonit_collectors::nut::DEFAULT_PORT.to_string());
    assert_eq!(
        defaut("nut", "request_timeout_seconds"),
        dumbmonit_collectors::nut::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
    );
    assert_eq!(defaut("nut", "ups"), "");
    // `collectors/openvpn/mod.rs`.
    assert_eq!(defaut("openvpn", "port"), dumbmonit_collectors::openvpn::DEFAULT_PORT.to_string());
    assert_eq!(
        defaut("openvpn", "request_timeout_seconds"),
        dumbmonit_collectors::openvpn::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
    );
    assert_eq!(describe("openvpn").default_port, dumbmonit_collectors::openvpn::DEFAULT_PORT);
    // `collectors/mikrotik/options.rs` : le port dépend du protocole.
    assert_eq!(defaut("mikrotik", "scheme"), "https");
    assert_eq!(defaut("mikrotik", "port"), "");
    assert_eq!(
        defaut("mikrotik", "request_timeout_seconds"),
        dumbmonit_collectors::mikrotik::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
    );
    assert_eq!(
        defaut("mikrotik", "max_interfaces"),
        dumbmonit_collectors::mikrotik::DEFAULT_MAX_INTERFACES.to_string()
    );
    assert_eq!(defaut("mikrotik", "interfaces"), "true");
    assert_eq!(
        defaut("paperless", "task_lookback_hours"),
        selfhosted::paperless::DEFAULT_TASK_LOOKBACK_HOURS.to_string()
    );
    assert_eq!(
        defaut("gitlab", "pipeline_lookback_hours"),
        selfhosted::gitlab::DEFAULT_PIPELINE_LOOKBACK_HOURS.to_string()
    );
    assert_eq!(defaut("gitlab", "watched_projects"), "");
    assert_eq!(
        defaut("push", "expected_interval"),
        crate::collectors::push::DEFAULT_EXPECTED_INTERVAL
    );
    assert_eq!(defaut("push", "grace"), crate::collectors::push::DEFAULT_GRACE);
    assert_eq!(defaut("push", "timezone"), crate::collectors::push::DEFAULT_TIMEZONE);
    assert_eq!(defaut("push", "schedule"), "");
    assert_eq!(defaut("push", "max_duration"), "");
    // `collectors/{traefik,caddy,nginx,apache,npm,domain}`.
    use dumbmonit_collectors::{apache, caddy, domain, nginx, npm, traefik};
    for (kind, port) in [
        ("traefik", traefik::DEFAULT_PORT),
        ("caddy", caddy::DEFAULT_PORT),
        ("npm", npm::DEFAULT_PORT),
        ("nginx", nginx::DEFAULT_PORT),
        ("apache", apache::DEFAULT_PORT),
    ] {
        assert_eq!(defaut(kind, "port"), port.to_string(), "« {kind} »");
        assert_eq!(describe(kind).default_port, port, "« {kind} »");
        assert_eq!(defaut(kind, "scheme"), "http");
        assert_eq!(
            defaut(kind, "request_timeout_seconds"),
            dumbmonit_collectors::observability::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
        );
    }
    assert_eq!(defaut("traefik", "metrics"), "true");
    assert_eq!(defaut("npm", "certificates"), "true");
    assert_eq!(defaut("npm", "tls_port"), npm::DEFAULT_TLS_PORT.to_string());
    assert_eq!(defaut("domain", "refresh_hours"), domain::DEFAULT_REFRESH_HOURS.to_string());
    assert_eq!(
        defaut("domain", "request_timeout_seconds"),
        domain::DEFAULT_REQUEST_TIMEOUT.as_secs().to_string()
    );
    // `collectors/blacklist/options.rs`.
    for kind in ["mail_blacklist", "ip_blacklist"] {
        use dumbmonit_collectors::blacklist as bl;
        assert_eq!(defaut(kind, "refresh_hours"), bl::DEFAULT_REFRESH_HOURS.to_string());
        assert_eq!(defaut(kind, "queries_per_second"), bl::DEFAULT_QUERIES_PER_SECOND.to_string());
        assert_eq!(
            defaut(kind, "query_timeout_seconds"),
            bl::DEFAULT_QUERY_TIMEOUT_SECONDS.to_string()
        );
    }
    assert_eq!(defaut("dns", "alert_on_change"), "false");
    // `collectors/nginx/mod.rs`.
    assert_eq!(defaut("nginx", "status_path"), nginx::DEFAULT_STATUS_PATH);
    assert_eq!(defaut("nginx", "plus_api"), "false");
    assert_eq!(defaut("nginx", "plus_api_version"), nginx::DEFAULT_PLUS_API_VERSION.to_string());
    // `collectors/apache/mod.rs`.
    assert_eq!(defaut("apache", "status_path"), apache::DEFAULT_STATUS_PATH);
    // `collectors/uptime/ntp/options.rs` : les sondes de disponibilité
    // n'exportent pas leurs défauts (ils ne sont lus que par la notice).
    assert_eq!(defaut("ntp", "port"), "123");
    assert_eq!(defaut("ntp", "offset_threshold_ms"), "100");
    assert_eq!(defaut("ssh", "port"), "22");
    assert_eq!(defaut("ssh", "host_key_type"), "auto");
    assert_eq!(defaut("ssh", "alert_on_change"), "false");
    assert_eq!(defaut("udp", "payload_encoding"), "text");
    for kind in ["imap", "pop3"] {
        assert_eq!(defaut(kind, "security"), "tls");
    }
    assert_eq!(defaut("snmp_oid", "port"), "161");
    assert_eq!(defaut("snmp_oid", "snmp_version"), "2c");
    assert_eq!(defaut("radius", "port"), "1812");
    assert_eq!(defaut("radius", "nas_identifier"), "dumbmonit");
    assert_eq!(defaut("grpc", "tls"), "false");
}

/// `credential_types` et `credentials` décrivent la même liste : l'ancienne
/// interface lit la première, la nouvelle la seconde.
#[test]
fn les_formes_didentifiant_sont_decrites_champ_par_champ() {
    for kind in KINDS_ENREGISTRES {
        let view = describe(kind);
        let kinds: Vec<&str> = view.credentials.iter().map(|c| c.kind).collect();
        assert_eq!(&kinds, view.credential_types, "« {kind} » : formes annoncées");
        for credential in view.credentials {
            assert!(!credential.label.is_empty(), "« {kind} » : forme sans libellé");
            let mut keys = std::collections::BTreeSet::new();
            for field in credential.fields {
                let contexte = format!("champ « {} » de « {kind} »", field.key);
                assert!(keys.insert(field.key), "{contexte} : déclaré deux fois");
                assert!(!field.label.is_empty(), "{contexte} : sans libellé");
                match field.input {
                    "select" => assert!(!field.choices.is_empty(), "{contexte} : sans choix"),
                    "text" | "password" => {
                        assert!(field.choices.is_empty(), "{contexte} : choix sur un texte")
                    }
                    other => panic!("{contexte} : type de champ inconnu « {other} »"),
                }
            }
            if credential.kind == "none" {
                assert!(credential.fields.is_empty(), "« {kind} » : « none » sans champ");
            } else {
                assert!(!credential.fields.is_empty(), "« {kind} » : forme sans champ");
            }
        }
    }
}

/// Un jeton Proxmox se saisit en deux cases ; les clés sont celles que
/// `dumbmonit_proto::Credential` sait recomposer.
#[test]
fn un_jeton_proxmox_se_saisit_en_deux_champs() {
    for kind in ["proxmox", "pbs", "pmg"] {
        let view = describe(kind);
        let token = view.credentials.iter().find(|c| c.kind == "api_token").unwrap();
        let keys: Vec<&str> = token.fields.iter().map(|f| f.key).collect();
        assert_eq!(keys, ["token_id", "secret"], "« {kind} »");
        assert!(token.fields[0].placeholder.contains('!'), "l'exemple montre user@realm!nom");
        assert_eq!(token.fields[1].input, "password");
        let credential: dumbmonit_proto::Credential = serde_json::from_value(serde_json::json!({
            "type": "api_token",
            "token_id": token.fields[0].placeholder,
            "secret": "8f3a1c9e-0000-4444-8888-aaaabbbbcccc",
        }))
        .unwrap();
        assert_eq!(
            credential,
            dumbmonit_proto::Credential::ApiToken {
                token: format!(
                    "{}=8f3a1c9e-0000-4444-8888-aaaabbbbcccc",
                    token.fields[0].placeholder
                )
            }
        );
    }
}

/// Les champs du formulaire RADIUS sont ceux que `Credential::Radius` lit.
#[test]
fn le_formulaire_radius_recompose_un_identifiant_radius() {
    let view = describe("radius");
    let keys: Vec<&str> = view.credentials[0].fields.iter().map(|f| f.key).collect();
    assert_eq!(keys, ["shared_secret", "username", "password"]);
    let mut object = serde_json::Map::new();
    object.insert("type".into(), "radius".into());
    for key in &keys {
        object.insert((*key).into(), format!("valeur-{key}").into());
    }
    let credential: dumbmonit_proto::Credential =
        serde_json::from_value(serde_json::Value::Object(object)).unwrap();
    assert_eq!(
        credential,
        dumbmonit_proto::Credential::Radius {
            shared_secret: "valeur-shared_secret".into(),
            username: "valeur-username".into(),
            password: "valeur-password".into(),
        }
    );
    for field in view.credentials[0].fields {
        assert_eq!(field.input == "password", field.key != "username", "{}", field.key);
    }
}

/// Texte d'une notice, étapes et mise en garde comprises.
fn notice(kind: &'static str) -> String {
    let view = describe(kind);
    let mut text = view.setup.steps.join("\n");
    text.push('\n');
    text.push_str(&view.setup.permissions.join("\n"));
    text.push('\n');
    text.push_str(view.setup.warning);
    text
}

/// Les tutoriels font créer un compte réservé, jamais réutiliser le compte
/// tout-puissant : le mot « root » ou « admin » n'a rien à y faire — sauf le
/// nom du groupe `administrators` que l'API de stockage de DSM exige, et qui
/// est justement expliqué.
#[test]
fn les_notices_font_creer_un_compte_dedie_et_ne_citent_jamais_le_compte_racine() {
    let attendus = [
        ("proxmox", "dumbmonit@pve"),
        ("pbs", "dumbmonit@pbs"),
        ("pdm", "dumbmonit@pdm"),
        ("pmg", "dumbmonit@pmg"),
        ("synology", "dumbmonit"),
        ("opnsense", "dumbmonit"),
        ("truenas", "dumbmonit"),
        ("redfish", "dumbmonit"),
        ("graylog", "dumbmonit"),
        ("mdaemon", "dumbmonit@example.com"),
        ("securitygateway", "dumbmonit"),
        ("redis", "dumbmonit"),
        ("mongodb", "dumbmonit"),
        ("rabbitmq", "dumbmonit"),
        ("mssql", "dumbmonit"),
        ("oracle", "dumbmonit"),
        ("elasticsearch", "dumbmonit"),
        ("kafka", "dumbmonit"),
        ("crowdsec", "dumbmonit"),
        ("traefik", "dumbmonit"),
        ("caddy", "dumbmonit"),
        ("npm", "dumbmonit@example.com"),
        ("kubernetes", "dumbmonit"),
        ("activedirectory", "svc-dumbmonit"),
        ("pfsense", "dumbmonit"),
        ("unraid", "dumbmonit"),
        ("veeam", "dumbmonit"),
        ("tailscale", "devices:core:read"),
        ("fortigate", "dumbmonit"),
        ("arubacentral", "dumbmonit"),
        ("sophos", "dumbmonit"),
        ("qnap", "dumbmonit"),
        ("ceph", "dumbmonit"),
        ("xcpng", "dumbmonit"),
        ("nutanix", "dumbmonit"),
        ("nextcloud", "token"),
        ("immich", "dumbmonit"),
        ("paperless", "dumbmonit"),
        ("jellyfin", "DumbMonit"),
        ("adguard", "dumbmonit"),
        ("nut", "[dumbmonit]"),
        ("mikrotik", "dumbmonit"),
        ("unifi", "dumbmonit"),
        ("homeassistant", "dumbmonit"),
        ("vsphere", "dumbmonit@vsphere.local"),
        ("agent", "token"),
    ];
    for (kind, dedie) in attendus {
        let text = notice(kind);
        assert!(text.contains(dedie), "« {kind} » ne nomme pas le compte dédié « {dedie} »");
        // Deux exceptions nommées, qui sont des privilèges et non des
        // comptes : le groupe `administrators` de DSM, et le privilège
        // « Local Administrator » que l'API REST de TrueNAS exige.
        let allowed = text
            .to_lowercase()
            .replace("administrators", "")
            .replace("local administrator", "")
            .replace("read-only administrator", "")
            .replace("global administrator", "")
            .replace("domain administrator", "")
            // L'interrupteur « Administrator » de Home Assistant, à laisser éteint.
            .replace("administrator toggle", "")
            // MongoDB range ses comptes dans la base `admin` : c'est un nom
            // de base, pas un compte.
            .replace("use admin", "")
            .replace("db: \"admin\"", "")
            .replace("than admin", "")
            // L'API d'administration de Caddy porte ce nom : c'est une
            // interface, pas un compte, et la notice la tient à l'écart.
            .replace("admin api", "")
            // L'onglet « Admin » de l'interface de RabbitMQ, où se gèrent
            // les utilisateurs : un menu, pas un compte.
            .replace("admin → users", "")
            // Rôles et types de comptes nommés pour être écartés ou
            // imposés par le produit : le rôle Backup Administrator que
            // Veeam réserve à certaines routes (et que DumbMonit ne demande
            // pas), le type « Administrator » obligatoire d'un compte
            // d'API Sophos, borné par son profil en lecture seule.
            .replace("backup administrator", "")
            .replace("type administrator", "")
            // Le domaine virtuel par défaut de FortiOS s'appelle `root` :
            // un nom de VDOM, pas un compte.
            .replace("set vdom \"root\"", "")
            // Les entrées de menu de FortiOS : System → Admin Profiles, et
            // « REST API Admin », le type de compte d'API.
            .replace("admin profiles", "")
            .replace("rest api admin", "")
            // Le menu « Device → Admin Roles » de PAN-OS, où se crée le
            // rôle en lecture seule : un menu, pas un compte.
            .replace("admin roles", "")
            // L'application « Admin Center » de Prism Central, où se
            // gèrent les identités : un menu, pas un compte.
            .replace("admin center", "");
        for word in allowed.split(|c: char| !c.is_alphanumeric()) {
            assert!(
                !matches!(word, "root" | "admin" | "administrator"),
                "la notice de « {kind} » cite « {word} » comme compte à utiliser"
            );
        }
    }
}

/// Les ports des notices : chaque type (sauf `dummy` et `ping`, qui n'ouvre que de
/// l'ICMP, sans port) en déclare au moins un,
/// avec un protocole, un sens et un usage ; un port facultatif dit quelle
/// option le demande (« Only with … »).
#[test]
fn les_ports_des_notices_sont_complets() {
    for kind in KINDS_ENREGISTRES.iter().filter(|kind| !matches!(**kind, "dummy" | "ping" | "tcp"))
    {
        let view = describe(kind);
        assert!(!view.setup.ports.is_empty(), "« {kind} » ne déclare aucun port");
        for port in view.setup.ports {
            assert!(port.port != 0, "« {kind} » : port 0");
            assert!(matches!(port.protocol, "tcp" | "udp"), "« {kind} » : {}", port.protocol);
            assert!(
                matches!(port.direction, "to_device" | "to_server"),
                "« {kind} » : sens {}",
                port.direction
            );
            assert!(!port.purpose.trim().is_empty(), "« {kind} » : port {} sans usage", port.port);
            assert!(
                !port.optional || port.purpose.starts_with("Only "),
                "« {kind} » : le port facultatif {} doit dire quelle option le demande",
                port.port
            );
        }
    }
}

/// Les droits des notices : un nom (une ligne), puis au plus une ligne qui dit
/// à quoi il sert ; une étape reste une phrase, pas un paragraphe.
#[test]
fn les_droits_et_les_etapes_des_notices_restent_courts() {
    for kind in KINDS_ENREGISTRES {
        let view = describe(kind);
        for permission in view.setup.permissions {
            let lines: Vec<&str> = permission.lines().collect();
            assert!(
                (1..=2).contains(&lines.len()) && lines.iter().all(|l| !l.trim().is_empty()),
                "« {kind} » : droit mal formé « {permission} »"
            );
            assert!(lines[0].len() <= 80, "« {kind} » : nom de droit trop long « {permission} »");
        }
        for step in view.setup.steps {
            let text = step.lines().next().unwrap_or_default();
            assert!(text.len() <= 220, "« {kind} » : étape trop longue « {text} »");
        }
    }
}

/// Les commandes des notices sont des lignes à copier telles quelles : pas
/// d'espace autour, pas de guillemets typographiques qu'un shell refuserait.
#[test]
fn les_commandes_des_notices_sont_copiables_telles_quelles() {
    for kind in KINDS_ENREGISTRES {
        for step in describe(kind).setup.steps {
            let mut lines = step.lines();
            let text = lines.next().unwrap_or_default();
            assert!(!text.trim().is_empty(), "« {kind} » : étape sans texte");
            for command in lines {
                assert_eq!(command, command.trim(), "« {kind} » : commande avec des espaces");
                assert!(!command.is_empty(), "« {kind} » : ligne de commande vide");
                assert!(
                    !command.contains(['“', '”', '‘', '’']),
                    "« {kind} » : guillemets typographiques dans « {command} »"
                );
            }
        }
    }
}

/// Les pages `docs/devices/*.md` reprennent les notices mot pour mot : le
/// tutoriel affiché dans l'application et celui de la documentation ne
/// doivent pas diverger. La comparaison ignore les retours à la ligne et
/// les accents de code Markdown.
#[test]
fn la_documentation_reprend_les_notices_mot_pour_mot() {
    let docs: &[(&str, &str)] = &[
        ("proxmox", include_str!("../../../../../docs/devices/proxmox.md")),
        ("pbs", include_str!("../../../../../docs/devices/pbs.md")),
        ("pdm", include_str!("../../../../../docs/devices/pdm.md")),
        ("pmg", include_str!("../../../../../docs/devices/pmg.md")),
        ("synology", include_str!("../../../../../docs/devices/synology.md")),
        ("opnsense", include_str!("../../../../../docs/devices/opnsense.md")),
        ("truenas", include_str!("../../../../../docs/devices/truenas.md")),
        ("redfish", include_str!("../../../../../docs/devices/redfish.md")),
        ("victoriametrics", include_str!("../../../../../docs/devices/victoriametrics.md")),
        ("victorialogs", include_str!("../../../../../docs/devices/victoriametrics.md")),
        ("loki", include_str!("../../../../../docs/devices/loki.md")),
        ("graylog", include_str!("../../../../../docs/devices/graylog.md")),
        ("mdaemon", include_str!("../../../../../docs/devices/mdaemon.md")),
        ("securitygateway", include_str!("../../../../../docs/devices/securitygateway.md")),
        ("nextcloud", include_str!("../../../../../docs/devices/nextcloud.md")),
        ("immich", include_str!("../../../../../docs/devices/immich.md")),
        ("paperless", include_str!("../../../../../docs/devices/paperless.md")),
        ("papra", include_str!("../../../../../docs/devices/papra.md")),
        ("jellyfin", include_str!("../../../../../docs/devices/jellyfin.md")),
        ("plex", include_str!("../../../../../docs/devices/plex.md")),
        ("gitlab", include_str!("../../../../../docs/devices/gitlab.md")),
        ("forgejo", include_str!("../../../../../docs/devices/forgejo.md")),
        ("sonarr", include_str!("../../../../../docs/devices/sonarr.md")),
        ("radarr", include_str!("../../../../../docs/devices/radarr.md")),
        ("lidarr", include_str!("../../../../../docs/devices/lidarr.md")),
        ("prowlarr", include_str!("../../../../../docs/devices/prowlarr.md")),
        ("qbittorrent", include_str!("../../../../../docs/devices/qbittorrent.md")),
        ("frigate", include_str!("../../../../../docs/devices/frigate.md")),
        ("keycloak", include_str!("../../../../../docs/devices/keycloak.md")),
        ("authentik", include_str!("../../../../../docs/devices/authentik.md")),
        ("vaultwarden", include_str!("../../../../../docs/devices/vaultwarden.md")),
        ("mailcow", include_str!("../../../../../docs/devices/mailcow.md")),
        ("pihole", include_str!("../../../../../docs/devices/pihole.md")),
        ("adguard", include_str!("../../../../../docs/devices/adguard.md")),
        ("nut", include_str!("../../../../../docs/devices/nut.md")),
        ("mikrotik", include_str!("../../../../../docs/devices/mikrotik.md")),
        ("unifi", include_str!("../../../../../docs/devices/unifi.md")),
        ("homeassistant", include_str!("../../../../../docs/devices/homeassistant.md")),
        ("vsphere", include_str!("../../../../../docs/devices/vsphere.md")),
        ("redis", include_str!("../../../../../docs/devices/redis.md")),
        ("mongodb", include_str!("../../../../../docs/devices/mongodb.md")),
        ("rabbitmq", include_str!("../../../../../docs/devices/rabbitmq.md")),
        ("mssql", include_str!("../../../../../docs/devices/mssql.md")),
        ("oracle", include_str!("../../../../../docs/devices/oracle.md")),
        ("elasticsearch", include_str!("../../../../../docs/devices/elasticsearch.md")),
        ("influxdb", include_str!("../../../../../docs/devices/influxdb.md")),
        ("kafka", include_str!("../../../../../docs/devices/kafka.md")),
        ("crowdsec", include_str!("../../../../../docs/devices/crowdsec.md")),
        ("traefik", include_str!("../../../../../docs/devices/traefik.md")),
        ("caddy", include_str!("../../../../../docs/devices/caddy.md")),
        ("nginx", include_str!("../../../../../docs/devices/nginx.md")),
        ("openvpn", include_str!("../../../../../docs/devices/openvpn.md")),
        ("apache", include_str!("../../../../../docs/devices/apache.md")),
        ("npm", include_str!("../../../../../docs/devices/npm.md")),
        ("domain", include_str!("../../../../../docs/devices/domain.md")),
        ("provider", include_str!("../../../../../docs/devices/provider.md")),
        ("mail_blacklist", include_str!("../../../../../docs/devices/mail_blacklist.md")),
        ("ip_blacklist", include_str!("../../../../../docs/devices/ip_blacklist.md")),
        ("kubernetes", include_str!("../../../../../docs/devices/kubernetes.md")),
        ("activedirectory", include_str!("../../../../../docs/devices/activedirectory.md")),
        ("pfsense", include_str!("../../../../../docs/devices/pfsense.md")),
        ("unraid", include_str!("../../../../../docs/devices/unraid.md")),
        ("veeam", include_str!("../../../../../docs/devices/veeam.md")),
        ("kopia", include_str!("../../../../../docs/devices/kopia.md")),
        ("duplicati", include_str!("../../../../../docs/devices/duplicati.md")),
        ("tailscale", include_str!("../../../../../docs/devices/tailscale.md")),
        ("fortigate", include_str!("../../../../../docs/devices/fortigate.md")),
        ("omada", include_str!("../../../../../docs/devices/omada.md")),
        ("arubacentral", include_str!("../../../../../docs/devices/arubacentral.md")),
        ("sophos", include_str!("../../../../../docs/devices/sophos.md")),
        ("qnap", include_str!("../../../../../docs/devices/qnap.md")),
        ("ceph", include_str!("../../../../../docs/devices/ceph.md")),
        ("minio", include_str!("../../../../../docs/devices/minio.md")),
        ("xcpng", include_str!("../../../../../docs/devices/xcpng.md")),
        ("nutanix", include_str!("../../../../../docs/devices/nutanix.md")),
        ("agent", include_str!("../../../../../docs/devices/agent.md")),
        ("agent_service", include_str!("../../../../../docs/devices/agent.md")),
        ("push", include_str!("../../../../../docs/devices/push.md")),
        ("webchange", include_str!("../../../../../docs/devices/webchange.md")),
        ("netconfig", include_str!("../../../../../docs/devices/netconfig.md")),
        ("website", include_str!("../../../../../docs/devices/website.md")),
        ("smtp", include_str!("../../../../../docs/devices/services.md")),
        ("postgres", include_str!("../../../../../docs/devices/services.md")),
        ("mysql", include_str!("../../../../../docs/devices/services.md")),
        ("mqtt", include_str!("../../../../../docs/devices/services.md")),
        ("websocket", include_str!("../../../../../docs/devices/services.md")),
        ("udp", include_str!("../../../../../docs/devices/services.md")),
        ("ssh", include_str!("../../../../../docs/devices/services.md")),
        ("imap", include_str!("../../../../../docs/devices/services.md")),
        ("pop3", include_str!("../../../../../docs/devices/services.md")),
        ("snmp_oid", include_str!("../../../../../docs/devices/services.md")),
        ("radius", include_str!("../../../../../docs/devices/services.md")),
        ("grpc", include_str!("../../../../../docs/devices/services.md")),
        ("http_steps", include_str!("../../../../../docs/devices/http-scenario.md")),
    ];
    fn flatten(text: &str) -> String {
        text.replace('`', "").split_whitespace().collect::<Vec<_>>().join(" ")
    }
    for (kind, doc) in docs {
        let flat = flatten(doc);
        let view = describe(kind);
        for step in view.setup.steps {
            let mut lines = step.lines();
            let text = flatten(lines.next().unwrap_or_default());
            assert!(flat.contains(&text), "docs/devices/{kind}.md ne reprend pas : {text}");
            for command in lines {
                assert!(doc.contains(command), "docs/devices/{kind}.md ne reprend pas : {command}");
            }
        }
        for permission in view.setup.permissions {
            for line in permission.lines() {
                let line = flatten(line);
                assert!(flat.contains(&line), "docs/devices/{kind}.md ne reprend pas : {line}");
            }
        }
        // Le bloc « Network ports to open » : une ligne de tableau par port, et la
        // phrase qui dit dans quel sens les connexions partent.
        for port in view.setup.ports {
            let row = format!(
                "| {} | {} | {}",
                port.port,
                port.protocol.to_uppercase(),
                flatten(port.purpose)
            );
            assert!(flat.contains(&row), "docs/devices/{kind}.md ne reprend pas : {row}");
            let way = if port.direction == "to_server" {
                "From the agent to the DumbMonit server"
            } else {
                "From the DumbMonit server (or its relay agent) to the device"
            };
            assert!(flat.contains(way), "docs/devices/{kind}.md ne dit pas : {way}");
        }
        let warning = flatten(view.setup.warning);
        assert!(flat.contains(&warning), "docs/devices/{kind}.md ne reprend pas : {warning}");
    }
}

/// Le formulaire des listes noires : une case par liste du catalogue, les
/// majeures cochées d'emblée, « sans clé » proposé en premier, et des
/// colonnes dont la clé est celle que lit `blacklist/lists.rs`.
#[test]
fn le_formulaire_des_listes_noires_suit_le_catalogue() {
    use dumbmonit_collectors::blacklist::lists;
    for kind in ["mail_blacklist", "ip_blacklist"] {
        let view = describe(kind);
        assert_eq!(view.credential_types[0], "none", "sans clé d'abord");
        assert!(view.credentials[0].label.contains("recommended"));
        assert!(view.setup.steps.iter().any(|s| s.contains("No API key is needed")));
        let option = |key: &str| view.options.iter().find(|o| o.key == key).unwrap();

        let checklist = option("lists").description();
        assert_eq!(checklist.input, "checklist");
        assert_eq!(checklist.items.len(), lists::LISTS.len());
        for (item, list) in checklist.items.iter().zip(lists::LISTS) {
            assert_eq!(item.value, list.id);
            assert!(!item.help.is_empty() && item.badges.len() == 2, "{}", list.id);
        }
        let majors: Vec<&str> = lists::LISTS.iter().filter(|l| l.major).map(|l| l.id).collect();
        assert_eq!(checklist.default, majors.join(","), "majeures cochées d'emblée");

        let rows = option("custom_lists").description();
        assert_eq!(rows.input, "rows");
        let keys: Vec<&str> = rows.columns.iter().map(|c| &*c.key).collect();
        assert_eq!(keys, ["zone", "subject", "major"]);
        // Le motif accepte une zone, refuse une adresse IP.
        let zone = rows.columns[0].pattern.to_string();
        assert!(zone.starts_with('^') && zone.ends_with('$') && rows.columns[0].unique);
    }
}

/// Les défauts affichés pour la santé des sites sont ceux que lit
/// `website/options.rs`.
#[test]
fn les_defauts_website_sont_ceux_du_collecteur() {
    use crate::website::options as o;
    let defaut = |cle: &str| {
        describe("website").options.iter().find(|opt| opt.key == cle).map(|opt| opt.default)
    };
    assert_eq!(
        defaut(o::OPTION_MIN_TEXT_CHARS),
        Some(o::DEFAULT_MIN_TEXT_CHARS.to_string().as_str())
    );
    assert_eq!(
        defaut(o::OPTION_MAX_REDIRECTS),
        Some(o::DEFAULT_MAX_REDIRECTS.to_string().as_str())
    );
    assert_eq!(defaut(o::OPTION_IGNORE_QUERY), Some("true"));
    assert_eq!(
        defaut(o::OPTION_CONFIRM_CHECKS),
        Some(o::DEFAULT_CONFIRM_CHECKS.to_string().as_str())
    );
    assert_eq!(
        defaut(o::OPTION_TEXT_DROP),
        Some(o::DEFAULT_TEXT_DROP_PERCENT.to_string().as_str())
    );
    for (cle, _) in crate::website::changes::WATCH_GROUPS {
        assert_eq!(defaut(cle), Some("true"), "{cle}");
    }
    assert_eq!(
        defaut(o::OPTION_CHANGE_CHECK),
        Some(o::DEFAULT_CHANGE_CHECK_MINUTES.to_string().as_str())
    );
    assert_eq!(
        defaut(o::OPTION_HOST_SPACING),
        Some(o::DEFAULT_HOST_SPACING_SECS.to_string().as_str())
    );
    for cle in [o::OPTION_EXPECT_TEXT, o::OPTION_USER_AGENT] {
        assert_eq!(defaut(cle), Some(""), "{cle}");
    }
    assert_eq!(defaut(o::OPTION_ALLOW_PRIVATE), Some("false"));
    assert_eq!(describe("website").credential_types, &["none"]);
}

/// Les défauts affichés pour la surveillance des sites sont ceux que lit
/// `webchange/options.rs`.
#[test]
fn les_defauts_de_la_surveillance_des_sites_sont_ceux_du_collecteur() {
    use crate::webchange::options as o;
    let defaut = |cle: &str| {
        describe("webchange").options.iter().find(|opt| opt.key == cle).map(|opt| opt.default)
    };
    assert_eq!(defaut(o::OPTION_SCOPE), Some("page"));
    assert_eq!(defaut(o::OPTION_MAX_PAGES), Some(o::DEFAULT_MAX_PAGES.to_string().as_str()));
    assert_eq!(
        defaut(o::OPTION_CHECK_INTERVAL),
        Some(o::DEFAULT_CHECK_INTERVAL_MINUTES.to_string().as_str())
    );
    assert_eq!(defaut(o::OPTION_KEEP_CHROME), Some("false"));
    for cle in [
        o::OPTION_PATH_PREFIX,
        o::OPTION_IGNORE,
        o::OPTION_CONTENT_SELECTOR,
        o::OPTION_EXCLUDE_SELECTOR,
        o::OPTION_IGNORE_QUERY,
        o::OPTION_USER_AGENT,
        o::OPTION_INSECURE_TLS,
        o::OPTION_ALLOW_PRIVATE,
    ] {
        assert!(defaut(cle).is_some(), "option « {cle} » absente du formulaire");
    }
    assert_eq!(describe("webchange").credential_types, &["none"]);
}

// La sonde ICMP ouvre un socket tokio : il lui faut un runtime.
#[tokio::test]
async fn seuls_les_types_qui_en_ont_besoin_declarent_un_prerequis() {
    assert_eq!(requirements("ping")[0].id, "icmp");
    for kind in ["snmp", "http", "tcp", "proxmox", "agent", "website"] {
        assert!(requirements(kind).is_empty(), "« {kind} » n'exige rien");
    }
}

/// Les blocs offerts à la copie sont ceux de la documentation, mot pour mot :
/// l'un ne peut pas être corrigé sans l'autre.
#[test]
fn les_blocs_compose_sont_ceux_de_la_documentation() {
    let doc = include_str!("../../../../../docs/install/docker.md");
    assert!(doc.contains(ICMP_SNIPPET.trim_end()), "le bloc « icmp » a divergé de docker.md");
    let ancre = "{#icmp-ping-without-a-capability}";
    assert!(doc.contains(ancre), "ancre {ancre} absente de docker.md");
}

#[test]
fn la_notice_du_ping_mentionne_le_sysctl_necessaire() {
    let view = describe("ping");
    let texte = view.setup.steps.join(" ") + view.setup.warning;
    assert!(texte.contains("net.ipv4.ping_group_range") || texte.contains("sysctls"));
    assert!(!texte.contains("NET_RAW"), "la capacité ne sert à rien à un utilisateur non root");
}

#[test]
fn la_sonde_http_annonce_les_identifiants_quelle_sait_envoyer() {
    let view = describe("http");
    assert!(view.credential_types.contains(&"username_password"));
    assert!(view.credential_types.contains(&"api_token"));
    for kind in ["tcp", "dns", "ping", "tls", "push", "domain"] {
        assert_eq!(describe(kind).credential_types, &["none"], "« {kind} » n'envoie rien");
    }
}

/// Une base de données ne se surveille pas anonymement : le formulaire ne
/// doit pas proposer « aucune authentification », qui échouerait à coup sûr.
#[test]
fn les_sondes_applicatives_annoncent_les_identifiants_quelles_savent_envoyer() {
    for kind in ["postgres", "mysql"] {
        assert_eq!(
            describe(kind).credential_types,
            &["username_password"],
            "« {kind} » exige un compte"
        );
    }
    for kind in ["smtp", "mqtt", "imap", "pop3"] {
        assert_eq!(describe(kind).credential_types, &["none", "username_password"], "{kind}");
    }
    assert_eq!(describe("websocket").credential_types, &["none", "username_password", "api_token"]);
    assert_eq!(describe("radius").credential_types, &["radius"]);
    assert_eq!(describe("grpc").credential_types, &["none"]);
    assert_eq!(
        describe("http_steps").credential_types,
        &["none", "username_password", "api_token"]
    );
    assert_eq!(describe("snmp_oid").credential_types, &["snmp_community", "snmp_v3"]);
}

/// Les défauts des sondes applicatives sont ceux de leurs `options.rs` : un
/// port faux dans le formulaire enverrait l'utilisateur chercher une panne
/// qui n'existe pas.
#[test]
fn les_defauts_des_sondes_applicatives_sont_ceux_des_collecteurs() {
    let defaut = |kind: &'static str, cle: &str| {
        describe(kind).options.iter().find(|o| o.key == cle).map(|o| o.default).unwrap()
    };
    assert_eq!(defaut("smtp", "security"), "starttls");
    assert_eq!(defaut("smtp", "helo_name"), dumbmonit_collectors::uptime::SMTP_DEFAULT_HELO);
    assert_eq!(defaut("postgres", "port"), "5432");
    assert_eq!(defaut("postgres", "database"), "postgres");
    assert_eq!(defaut("mysql", "port"), "3306");
    assert_eq!(defaut("mysql", "database"), "", "MySQL n'exige pas qu'on nomme une base");
    for kind in ["postgres", "mysql"] {
        assert_eq!(
            defaut(kind, "query"),
            dumbmonit_collectors::uptime::SQL_DEFAULT_QUERY,
            "« {kind} » : la requête par défaut ne doit lire aucune table"
        );
        assert_eq!(defaut(kind, "sslmode"), "prefer");
    }
    assert_eq!(defaut("mqtt", "tls"), "false");
    assert_eq!(defaut("mqtt", "expect_message"), "false");
}
