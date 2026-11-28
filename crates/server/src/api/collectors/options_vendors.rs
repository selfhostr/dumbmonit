//! Identifiants et réglages des fournisseurs, domaines, listes de blocage,
//! pare-feu, NAS, hyperviseurs, outils de sauvegarde et réseaux d'éditeurs.

use super::*;

/// Options lues par `collectors/uptime/provider/mod.rs`.
pub(super) const PROVIDER_OPTIONS: &[OptionView] = &[
    text(
        "component",
        "Component",
        "Only watch the components (or, in a feed, the entries) whose name contains this text. Empty: the provider as a whole.",
        "Workers",
        "",
    ),
    select(
        "fail_on",
        "Fail on",
        "Which level counts as down: minor (any degradation), major (a partial or major outage) or critical (a major outage only).",
        "minor",
        &["minor", "major", "critical"],
    ),
    number(
        "refresh_seconds",
        "Refresh interval (seconds)",
        "How long the provider's answer is reused, from 30 to 3600. Monitors and status pages on the same provider share one request.",
        "60",
        "60",
    ),
    number(
        "window_hours",
        "Feed window (hours)",
        "RSS and Atom feeds only: an entry older than this no longer counts as an open incident, from 1 to 168.",
        "24",
        "24",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/domain/mod.rs`.
pub(super) const DOMAIN_OPTIONS: &[OptionView] = &[
    text(
        "rdap_server",
        "RDAP server",
        "The RDAP address of the registry, for an extension the IANA does not list. Empty: found in the IANA bootstrap file.",
        "https://rdap.nic.ch/",
        "",
    ),
    number(
        "refresh_hours",
        "Refresh interval (hours)",
        "How often the registry is asked, from 1 to 168. The days left are recomputed at every check in between.",
        "12",
        "12",
    ),
    number(
        "request_timeout_seconds",
        "Timeout per request (seconds)",
        "Time allowed for each call to IANA or to the registry, from 1 to 120.",
        "10",
        "10",
    ),
];

/// Pas de clé : le choix par défaut, et le bon pour presque tout le monde.
pub(super) const BLOCKLIST_NO_KEY: CredentialView = CredentialView {
    kind: "none",
    label: "No key (recommended)",
    help: "No key is needed. The blocklists are queried through this server's DNS resolver, or the one set under Resolver. Only Spamhaus can refuse, and only when that resolver is a public one (Google, Cloudflare, Quad9…): the monitor then says \"Spamhaus refuses this resolver\" and every other list keeps working.",
    fields: &[],
};

/// Clé du Data Query Service de Spamhaus (`collectors/blacklist/options.rs`).
pub(super) const SPAMHAUS_DQS: CredentialView = CredentialView {
    kind: "api_token",
    label: "Spamhaus DQS key (optional)",
    help: "Only useful if Spamhaus refuses your resolver. A free Spamhaus Data Query Service key (low, non-commercial volumes) makes Spamhaus answer through any resolver, public ones included. It changes nothing for the other lists.",
    fields: &[cred_secret(
        "token",
        "DQS key",
        "The key shown in your Spamhaus account. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const BLOCKLIST_RESOLVER: OptionView = text(
    "resolver",
    "Resolver",
    "IP address of the recursive resolver to query, port optional. Empty: the system resolver, which is fine. Spamhaus alone refuses public resolvers (8.8.8.8, 1.1.1.1, 9.9.9.9) and those that forward to them; the other lists do not mind.",
    "10.0.0.53",
    "",
);

/// Les majeures du catalogue, cochées d'emblée sur un nouveau moniteur (un test
/// vérifie l'accord avec `lists::LISTS`).
pub(super) const BLOCKLIST_MAJOR_LISTS: &str = "spamhaus_zen,barracuda,spamcop,spamhaus_dbl";

/// Les cases du catalogue : nom, type de sujet, importance et description.
pub(super) fn blocklist_items() -> Vec<OptionItem> {
    use dumbmonit_collectors::blacklist::lists::{LISTS, Subject};
    LISTS
        .iter()
        .map(|list| OptionItem {
            value: list.id.into(),
            label: list.name.into(),
            help: list.description.into(),
            badges: vec![
                if list.subject == Subject::Ip { "IP address" } else { "Domain" }.into(),
                if list.major { "Major" } else { "Minor" }.into(),
            ],
        })
        .collect()
}

pub(super) const BLOCKLIST_LISTS: OptionView = OptionView {
    key: "lists",
    label: "Blocklists",
    help: "The lists to query. Major lists are the ones whose listing blocks a large share of mail and raises the critical alert; the others are shown as a warning. IP lists check addresses, domain lists check the mail domain (Mail server reputation only).",
    placeholder: "",
    default: BLOCKLIST_MAJOR_LISTS,
    required: false,
    input: "checklist",
    choices: &[],
    group: "",
    items: blocklist_items,
    columns: no_columns,
};

pub(super) fn blocklist_custom_columns() -> Vec<OptionColumn> {
    vec![
        OptionColumn {
            key: "zone".into(),
            label: "DNS zone".into(),
            placeholder: "dnsbl.example.org".into(),
            input: "text".into(),
            pattern: r"^[a-z0-9_]([a-z0-9_-]{0,61}[a-z0-9_])?(\.[a-z0-9_]([a-z0-9_-]{0,61}[a-z0-9_])?)*\.[a-z0-9-]*[a-z][a-z0-9-]*$".into(),
            invalid: "Enter a DNS zone name such as dnsbl.example.org, not an IP address.".into(),
            unique: true,
            ..OptionColumn::default()
        },
        OptionColumn {
            key: "subject".into(),
            label: "Type".into(),
            input: "select".into(),
            choices: vec!["ip".into(), "domain".into()],
            ..OptionColumn::default()
        },
        OptionColumn {
            key: "major".into(),
            label: "Major".into(),
            input: "boolean".into(),
            ..OptionColumn::default()
        },
    ]
}

pub(super) const BLOCKLIST_CUSTOM_LISTS: OptionView = OptionView {
    key: "custom_lists",
    label: "Custom lists",
    help: "Your own DNSBL zones, queried in addition to the ticked ones, at most 20. Type ip: the zone is asked about each address (1.2.0.192.zone); type domain: about the mail domain. Tick Major if a listing there should raise the critical alert.",
    placeholder: "",
    default: "",
    required: false,
    input: "rows",
    choices: &[],
    group: "",
    items: no_items,
    columns: blocklist_custom_columns,
};

pub(super) const BLOCKLIST_REFRESH: OptionView = number(
    "refresh_hours",
    "Refresh interval (hours)",
    "How often the blocklists are asked, from 1 to 168. In between, the last result is shown again at every check.",
    "6",
    "6",
);

pub(super) const BLOCKLIST_RATE: OptionView = number(
    "queries_per_second",
    "Queries per second",
    "Pace of the queries, from 1 to 50. Blocklists ban resolvers that sweep them.",
    "10",
    "10",
);

pub(super) const BLOCKLIST_TIMEOUT: OptionView = number(
    "query_timeout_seconds",
    "Timeout per query (seconds)",
    "From 1 to 30. A blocklist that does not answer in time is shown as unknown, never as clean.",
    "5",
    "5",
);

/// Options lues par `collectors/blacklist` pour un serveur mail.
pub(super) const MAIL_BLACKLIST_OPTIONS: &[OptionView] = &[
    BLOCKLIST_RESOLVER,
    BLOCKLIST_LISTS,
    BLOCKLIST_CUSTOM_LISTS,
    text(
        "extra_addresses",
        "Extra addresses",
        "Sending addresses to check as well, separated by commas: an outgoing relay that is not an MX.",
        "198.51.100.25",
        "",
    ),
    BLOCKLIST_REFRESH,
    BLOCKLIST_RATE,
    BLOCKLIST_TIMEOUT,
];

/// Options lues par `collectors/blacklist` pour une plage d'adresses.
pub(super) const IP_BLACKLIST_OPTIONS: &[OptionView] = &[
    BLOCKLIST_RESOLVER,
    BLOCKLIST_LISTS,
    BLOCKLIST_CUSTOM_LISTS,
    BLOCKLIST_REFRESH,
    BLOCKLIST_RATE,
    BLOCKLIST_TIMEOUT,
];

/// Délai par requête des équipements commerciaux lus par leur API
/// (`collectors/{pfsense,unraid,veeam,tailscale,fortigate,sophos,omada}`, défaut de
/// `selfhosted/options.rs`).
pub(super) const VENDOR_TIMEOUT: OptionView = number(
    "request_timeout_seconds",
    "Timeout per request (seconds)",
    "Time allowed for each call, from 1 to 120.",
    "15",
    "15",
);

pub(super) const SELF_SIGNED_TLS: OptionView = insecure_tls(
    "The device ships a self-signed certificate: enable this unless you installed one DumbMonit trusts.",
);

/// Clé du paquet REST API de pfSense, envoyée en `X-API-Key`.
pub(super) const PFSENSE_KEY: CredentialView = CredentialView {
    kind: "api_token",
    label: "REST API key",
    help: "The key created for the dumbmonit user: it carries only that user's read privileges.",
    fields: &[cred_secret(
        "token",
        "API key",
        "Printed once by the key creation of step 3. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const PFSENSE_OPTIONS: &[OptionView] =
    &[app_scheme("https"), app_port("443"), SELF_SIGNED_TLS, VENDOR_TIMEOUT];

/// Clé d'API Unraid au rôle Viewer, envoyée en `x-api-key`.
pub(super) const UNRAID_KEY: CredentialView = CredentialView {
    kind: "api_token",
    label: "API key",
    help: "A key with the Viewer role: it reads everything and can change nothing.",
    fields: &[cred_secret(
        "token",
        "API key",
        "Shown once when the key is created. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const UNRAID_OPTIONS: &[OptionView] = &[
    app_scheme("http"),
    app_port("80"),
    insecure_tls(
        "For a web interface served over HTTPS with a self-signed certificate: enable this if the connection is refused for that reason.",
    ),
    VENDOR_TIMEOUT,
];

/// Compte Windows au rôle Veeam Backup Viewer (`collectors/veeam`).
pub(super) const VEEAM_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "User name and password",
    help: "The account given the Veeam Backup Viewer role, in the form HOST\\user or DOMAIN\\user.",
    fields: &[
        cred_text(
            "username",
            "User name",
            "The account given the Veeam Backup Viewer role.",
            "VBR01\\dumbmonit",
        ),
        cred_secret("password", "Password", "", "", true),
    ],
};

pub(super) const VEEAM_OPTIONS: &[OptionView] = &[
    app_scheme("https"),
    app_port("9419"),
    text(
        "api_version",
        "API version",
        "Sent in the x-api-version header. 1.1-rev0 is accepted from Veeam Backup & Replication 12.0 on; change it only if the server refuses it.",
        "1.1-rev0",
        "1.1-rev0",
    ),
    SELF_SIGNED_TLS,
    VENDOR_TIMEOUT,
    DEVICE_STALE_DAYS,
];

/// Compte Windows de WinRM (`collectors/winrm`) : NTLM par défaut, Basic en HTTPS.
pub(super) const WINRM_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "Windows account",
    help: "A local or domain account in the Remote Management Users group, as dumbmonit, DOMAIN\\dumbmonit or dumbmonit@domain.",
    fields: &[
        cred_text(
            "username",
            "User name",
            "The account created on the server, as dumbmonit (local) or DOMAIN\\dumbmonit.",
            "CORP\\dumbmonit",
        ),
        cred_secret("password", "Password", "", "", true),
    ],
};

pub(super) const WINRM_OPTIONS: &[OptionView] = &[
    boolean(
        "allow_unencrypted",
        "Allow unencrypted traffic",
        "Use HTTP on port 5985 instead of HTTPS on port 5986. Not recommended: the server must allow unencrypted WinRM, and everything that crosses the network, the login included, can be read. Leave it off whenever an HTTPS listener exists.",
        false,
    ),
    number(
        "port",
        "Port",
        "Used if the address does not give a port. 5986 for HTTPS, 5985 with unencrypted traffic.",
        "5986",
        "",
    ),
    select(
        "auth",
        "Authentication",
        "NTLM works with local and domain accounts. Basic must be enabled on the server and is only accepted over HTTPS. Kerberos is not supported.",
        "ntlm",
        &["ntlm", "basic"],
    ),
    SELF_SIGNED_TLS,
    VENDOR_TIMEOUT,
    text(
        "services",
        "Services to follow",
        "Service names separated by commas. Each gets an up/down series, and one missing from the server counts as stopped. Every automatic service that is stopped is reported anyway.",
        "Spooler, W3SVC",
        "",
    ),
    text(
        "ignore_services",
        "Stopped services to ignore",
        "Automatic services allowed to be stopped, separated by commas; a * at the start or the end is a wildcard. Updaters and delayed-start services such as gupdate are already ignored.",
        "MSSQL*, Backup*",
        "",
    ),
    text(
        "event_logs",
        "Event logs to count",
        "Classic logs whose recent errors are counted (audit failures for Security), separated by commas. Write none to turn it off.",
        "System,Application",
        "System,Application",
    ),
    number(
        "event_minutes",
        "Event window (minutes)",
        "Errors logged during this many minutes are counted, from 1 to 1440.",
        "60",
        "60",
    ),
];

/// Hôte Hyper-V (`collectors/winrm/hyperv.rs`) : le transport de WinRM, sans les
/// réglages des services ni du journal d'événements.
pub(super) const HYPERV_OPTIONS: &[OptionView] = &[
    boolean(
        "allow_unencrypted",
        "Allow unencrypted traffic",
        "Use HTTP on port 5985 instead of HTTPS on port 5986. Not recommended: the server must allow unencrypted WinRM, and everything that crosses the network, the login included, can be read. Leave it off whenever an HTTPS listener exists.",
        false,
    ),
    number(
        "port",
        "Port",
        "Used if the address does not give a port. 5986 for HTTPS, 5985 with unencrypted traffic.",
        "5986",
        "",
    ),
    select(
        "auth",
        "Authentication",
        "NTLM works with local and domain accounts. Basic must be enabled on the server and is only accepted over HTTPS. Kerberos is not supported.",
        "ntlm",
        &["ntlm", "basic"],
    ),
    SELF_SIGNED_TLS,
    VENDOR_TIMEOUT,
];

/// Identifiants du serveur Kopia (`--server-username` / `--server-password`),
/// envoyés en authentification HTTP Basic (`collectors/kopia`).
pub(super) const KOPIA_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "Server user name and password",
    help: "The user name and password the Kopia server was started with.",
    fields: &[
        cred_text(
            "username",
            "User name",
            "The --server-username of the Kopia server.",
            "dumbmonit",
        ),
        cred_secret("password", "Password", "The --server-password of the Kopia server.", "", true),
    ],
};

pub(super) const KOPIA_OPTIONS: &[OptionView] =
    &[app_scheme("https"), app_port("51515"), SELF_SIGNED_TLS, VENDOR_TIMEOUT];

/// Mot de passe de l'interface web de Duplicati 2.1+ (`collectors/duplicati`) :
/// Duplicati n'a pas de compte, seulement ce mot de passe.
pub(super) const DUPLICATI_PASSWORD: CredentialView = CredentialView {
    kind: "api_token",
    label: "Web interface password",
    help: "Duplicati has no user accounts: the password of its web interface is all it asks for.",
    fields: &[cred_secret(
        "token",
        "Web interface password",
        "Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const DUPLICATI_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("8200"), APP_TLS, VENDOR_TIMEOUT];

/// Client OAuth Tailscale à la portée `devices:core:read`.
pub(super) const TAILSCALE_OAUTH: CredentialView = CredentialView {
    kind: "username_password",
    label: "OAuth client",
    help: "Recommended: a client limited to the devices:core:read scope, which can change nothing.",
    fields: &[
        cred_text(
            "username",
            "Client ID",
            "The ID of the OAuth client created in step 1.",
            "k123AbCDeF11CNTRL",
        ),
        cred_secret(
            "password",
            "Client secret",
            "The tskey-client-… secret, shown once. Stored encrypted, never shown again.",
            "tskey-client-…",
            true,
        ),
    ],
};

/// Jeton d'accès à l'API Tailscale (`tskey-api-…`).
pub(super) const TAILSCALE_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "API access token",
    help: "Works too, but carries every right of the user who created it and expires after 90 days at most.",
    fields: &[cred_secret(
        "token",
        "API access token",
        "The tskey-api-… token. Stored encrypted, never shown again.",
        "tskey-api-…",
        true,
    )],
};

pub(super) const TAILSCALE_OPTIONS: &[OptionView] = &[
    text(
        "tailnet",
        "Tailnet",
        "Leave empty for the tailnet of the credentials, or enter the tailnet name shown in the Tailscale console.",
        "example.com",
        "",
    ),
    text(
        "watch",
        "Devices to keep online",
        "Comma-separated device names or tags (tag:server) that must stay online, or all. Empty: every tagged device.",
        "nas, tag:router",
        "",
    ),
    VENDOR_TIMEOUT,
    DEVICE_STALE_DAYS,
];

/// Client d'API personnel de HPE GreenLake pour Aruba Central, échangé contre un
/// jeton de deux heures (`collectors/arubacentral`).
pub(super) const ARUBACENTRAL_CLIENT: CredentialView = CredentialView {
    kind: "username_password",
    label: "API client",
    help: "Recommended: DumbMonit exchanges the client for an access token and renews it by itself.",
    fields: &[
        cred_text(
            "username",
            "Client ID",
            "The client ID of the personal API client created in step 2.",
            "",
        ),
        cred_secret(
            "password",
            "Client secret",
            "Shown once when the client is created. Stored encrypted, never shown again.",
            "",
            true,
        ),
    ],
};

/// Jeton d'accès déjà obtenu : il expire au bout de deux heures.
pub(super) const ARUBACENTRAL_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "Access token",
    help: "Works for a test, but expires after two hours and DumbMonit cannot renew it.",
    fields: &[cred_secret(
        "token",
        "Access token",
        "An access token of your Central instance. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const ARUBACENTRAL_OPTIONS: &[OptionView] = &[
    text(
        "token_address",
        "Sign-in host",
        "The HPE GreenLake host that issues the access tokens. Change it only if HPE gives you another.",
        "sso.common.cloud.hpe.com",
        "sso.common.cloud.hpe.com",
    ),
    VENDOR_TIMEOUT,
];

/// Jeton d'un administrateur REST API FortiOS au profil en lecture seule.
pub(super) const FORTIGATE_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "API token",
    help: "The token of the dumbmonit REST API user, sent as a bearer token.",
    fields: &[cred_secret(
        "token",
        "API token",
        "Printed once by execute api-user generate-key. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const FORTIGATE_OPTIONS: &[OptionView] = &[
    number(
        "port",
        "Port",
        "Used if the address does not give a port: the HTTPS port of the administration interface.",
        "443",
        "443",
    ),
    text(
        "vdom",
        "VDOM",
        "The virtual domain to read. Empty: the one the REST API user belongs to.",
        "root",
        "",
    ),
    SELF_SIGNED_TLS,
    VENDOR_TIMEOUT,
];

/// Client OpenAPI d'Omada (mode « Client », rôle Observateur).
pub(super) const OMADA_CLIENT: CredentialView = CredentialView {
    kind: "username_password",
    label: "OpenAPI client",
    help: "The client created for DumbMonit under Open API, in Client mode with the Viewer role.",
    fields: &[
        cred_text("username", "Client ID", "The Client ID shown when the app was created.", ""),
        cred_secret(
            "password",
            "Client secret",
            "The Client Secret, shown once. Stored encrypted, never shown again.",
            "",
            true,
        ),
    ],
};

pub(super) const OMADA_OPTIONS: &[OptionView] = &[
    number(
        "port",
        "Port",
        "Used if the address does not give a port: the HTTPS port of the controller (8043 for the software controller, 443 for a hardware one).",
        "8043",
        "8043",
    ),
    text(
        "omadac_id",
        "Controller ID",
        "The Omada ID shown under Open API. Empty: read from the controller, which publishes it.",
        "a3f1c0d2e4b5967788990011aabbccdd",
        "",
    ),
    text(
        "site",
        "Site",
        "The name or ID of the only site to read. Empty: every site.",
        "Default",
        "",
    ),
    SELF_SIGNED_TLS,
    VENDOR_TIMEOUT,
];

/// Compte au profil d'accès en lecture seule (`collectors/sophos`).
pub(super) const SOPHOS_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "User name and password",
    help: "The dumbmonit user, whose device access profile is read-only everywhere.",
    fields: &[
        cred_text(
            "username",
            "User name",
            "The read-only user created for DumbMonit.",
            "dumbmonit",
        ),
        cred_secret("password", "Password", "", "", true),
    ],
};

pub(super) const SOPHOS_OPTIONS: &[OptionView] = &[
    number(
        "port",
        "Port",
        "Used if the address does not give a port: the administration port, 4444 by default.",
        "4444",
        "4444",
    ),
    SELF_SIGNED_TLS,
    VENDOR_TIMEOUT,
];

/// Compte local du NAS QNAP (`collectors/qnap`), lu par les pages CGI de QTS.
pub(super) const QNAP_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "User name and password",
    help: "The dumbmonit account created on the NAS. The password is sent base64-encoded to the login page, as the web interface does.",
    fields: &[
        cred_text("username", "User name", "The account created for DumbMonit.", "dumbmonit"),
        cred_secret("password", "Password", "", "", true),
    ],
};

pub(super) const QNAP_OPTIONS: &[OptionView] = &[
    app_scheme("https"),
    number(
        "port",
        "Port",
        "Used if the address does not give a port: 443 over HTTPS, and 8080 over HTTP (enter it, QTS does not use 80).",
        "443",
        "443",
    ),
    SELF_SIGNED_TLS,
    VENDOR_TIMEOUT,
];

/// Utilisateur du tableau de bord Ceph au rôle `read-only` (`collectors/ceph`).
pub(super) const CEPH_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "Dashboard user and password",
    help: "The dumbmonit user of the Ceph dashboard, with the read-only role.",
    fields: &[
        cred_text(
            "username",
            "User name",
            "The dashboard user created for DumbMonit.",
            "dumbmonit",
        ),
        cred_secret("password", "Password", "", "", true),
    ],
};

pub(super) const CEPH_OPTIONS: &[OptionView] = &[
    app_scheme("https"),
    number(
        "port",
        "Port",
        "Used if the address does not give a port: the dashboard port, 8443 over HTTPS and 8080 over HTTP.",
        "8443",
        "8443",
    ),
    SELF_SIGNED_TLS,
    VENDOR_TIMEOUT,
];

/// Jeton JWT de `mc admin prometheus generate`, envoyé en `Authorization: Bearer`.
pub(super) const MINIO_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "Prometheus token",
    help: "The bearer_token printed by mc admin prometheus generate. It opens the metrics pages only.",
    fields: &[cred_secret(
        "token",
        "Prometheus token",
        "The JWT printed after bearer_token. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const MINIO_OPTIONS: &[OptionView] = &[
    app_scheme("http"),
    number(
        "port",
        "Port",
        "Used if the address does not give a port: the S3 API port, 9000 (not the console, 9001).",
        "9000",
        "9000",
    ),
    insecure_tls(
        "For a server with TLS enabled and a self-signed or private certificate: enable this if the connection is refused for that reason.",
    ),
    VENDOR_TIMEOUT,
];

/// Sujet XAPI de `collectors/xcpng` : un compte de l'annuaire du pool (Active
/// Directory, ou PAM sur les hôtes), au rôle intégré `read-only`.
pub(super) const XCPNG_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "User name and password",
    help: "The dumbmonit account, registered on the pool as a subject with the read-only role.",
    fields: &[
        cred_text("username", "User name", "As the pool's directory knows it.", "dumbmonit"),
        cred_secret("password", "Password", "", "", true),
    ],
};

pub(super) const XCPNG_OPTIONS: &[OptionView] = &[
    number("port", "Port", "Used if the address does not give a port.", "443", "443"),
    SELF_SIGNED_TLS,
    VENDOR_TIMEOUT,
];

/// Utilisateur Prism Central de `collectors/nutanix`.
pub(super) const NUTANIX_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "User name and password",
    help: "The dumbmonit local user of Prism Central, with the Prism Viewer role.",
    fields: &[
        cred_text("username", "User name", "", "dumbmonit"),
        cred_secret("password", "Password", "", "", true),
    ],
};

pub(super) const NUTANIX_OPTIONS: &[OptionView] = &[
    number("port", "Port", "Used if the address does not give a port.", "9440", "9440"),
    SELF_SIGNED_TLS,
    VENDOR_TIMEOUT,
];

pub(super) const OPENVPN_NONE: CredentialView = CredentialView {
    kind: "none",
    label: "No password",
    help: "Only for a management interface started without a password-file argument.",
    fields: &[],
};

pub(super) const OPENVPN_PASSWORD: CredentialView = CredentialView {
    kind: "api_token",
    label: "Management password",
    help: "The shared password set by the third argument of the server's \"management\" directive.",
    fields: &[cred_secret(
        "token",
        "Management password",
        "The one word every management client sends; OpenVPN has no account for this interface.",
        "",
        true,
    )],
};

/// Options lues par `collectors/openvpn/mod.rs`.
pub(super) const OPENVPN_OPTIONS: &[OptionView] = &[
    number(
        "port",
        "Port",
        "The management interface's port, used if the address does not give one.",
        "7505",
        "7505",
    ),
    number(
        "request_timeout_seconds",
        "Timeout per exchange (seconds)",
        "Time allowed for the connection and each answer, from 1 to 60.",
        "10",
        "10",
    ),
];
