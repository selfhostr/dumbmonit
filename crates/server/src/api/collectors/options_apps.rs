//! Identifiants et réglages des serveurs d'observabilité, des applications
//! auto-hébergées, des résolveurs DNS, des onduleurs, du réseau (MikroTik,
//! UniFi), de Home Assistant et de vSphere.

use super::*;

/// Jeton d'accès Graylog : envoyé en « basic », le jeton comme nom
/// d'utilisateur et le mot `token` comme mot de passe
/// (`collectors/observability/client.rs`).
pub(super) const GRAYLOG_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "Access token (recommended)",
    help: "A token of the dedicated Graylog user, created from its Edit tokens page.",
    fields: &[cred_secret(
        "token",
        "Access token",
        "Graylog shows it once. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const GRAYLOG_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "User name / password",
    help: "The dedicated Graylog user itself, sent as HTTP basic authentication.",
    fields: &[
        cred_text("username", "User name", "", "dumbmonit"),
        cred_secret("password", "Password", "", "", true),
    ],
};

/// Protocole des serveurs de journaux et de métriques : en clair par défaut,
/// comme ils écoutent tant qu'aucun proxy inverse n'est placé devant.
pub(super) const OBSERVABILITY_SCHEME: OptionView = select(
    "scheme",
    "Protocol",
    "These servers listen over plain HTTP unless you put them behind a reverse proxy with TLS. HTTPS then.",
    "http",
    &["http", "https"],
);

pub(super) const OBSERVABILITY_TIMEOUT: OptionView = number(
    "request_timeout_seconds",
    "Timeout per request (seconds)",
    "Time allowed for each call, from 1 to 120.",
    "10",
    "10",
);

pub(super) const fn observability_port(default: &'static str) -> OptionView {
    number("port", "Port", "Used if the address does not give a port.", default, default)
}

pub(super) const OBSERVABILITY_TLS: OptionView = insecure_tls(
    "For a reverse proxy with a self-signed or private certificate: enable this if the connection is refused for that reason.",
);

/// Options lues par `collectors/observability/options.rs`, une liste par port.
pub(super) const VICTORIAMETRICS_OPTIONS: &[OptionView] =
    &[OBSERVABILITY_SCHEME, observability_port("8428"), OBSERVABILITY_TLS, OBSERVABILITY_TIMEOUT];
pub(super) const VICTORIALOGS_OPTIONS: &[OptionView] =
    &[OBSERVABILITY_SCHEME, observability_port("9428"), OBSERVABILITY_TLS, OBSERVABILITY_TIMEOUT];
pub(super) const LOKI_OPTIONS: &[OptionView] =
    &[OBSERVABILITY_SCHEME, observability_port("3100"), OBSERVABILITY_TLS, OBSERVABILITY_TIMEOUT];
pub(super) const GRAYLOG_OPTIONS: &[OptionView] =
    &[OBSERVABILITY_SCHEME, observability_port("9000"), OBSERVABILITY_TLS, OBSERVABILITY_TIMEOUT];

/// Applications auto-hébergées (`collectors/selfhosted/options.rs`) : protocole
/// et port propres à chaque produit, délai et certificat communs.
pub(super) const fn app_scheme(default: &'static str) -> OptionView {
    select(
        "scheme",
        "Protocol",
        "Used when the address does not start with http:// or https://.",
        default,
        &["http", "https"],
    )
}

pub(super) const fn app_port(default: &'static str) -> OptionView {
    number(
        "port",
        "Port",
        "Used if the address does not give a port. Choosing the other protocol without a port uses 80 or 443.",
        default,
        default,
    )
}

pub(super) const APP_TLS: OptionView = insecure_tls(
    "For a self-signed or private certificate: enable this if the connection is refused for that reason.",
);

pub(super) const APP_TIMEOUT: OptionView = number(
    "request_timeout_seconds",
    "Timeout per request (seconds)",
    "Time allowed for each call, from 1 to 120. Photo and file counts are computed on every call on a large instance.",
    "15",
    "15",
);

pub(super) const DEVICE_STALE_DAYS: OptionView = number(
    "device_stale_days",
    "Device staleness (days)",
    "A device (phone, tablet, desktop…) with no connection and no backup for longer than this is reported stale. From 1 to 90.",
    "3",
    "3",
);

pub(super) const NEXTCLOUD_OPTIONS: &[OptionView] =
    &[app_scheme("https"), app_port("443"), APP_TLS, APP_TIMEOUT];
pub(super) const IMMICH_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("2283"), APP_TLS, APP_TIMEOUT, DEVICE_STALE_DAYS];
pub(super) const PAPERLESS_OPTIONS: &[OptionView] = &[
    app_scheme("http"),
    app_port("8000"),
    APP_TLS,
    APP_TIMEOUT,
    number(
        "task_lookback_hours",
        "Failed task window (hours)",
        "Failed tasks newer than this, and not dismissed in Paperless, are counted and named. From 1 to 720.",
        "24",
        "24",
    ),
];
pub(super) const JELLYFIN_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("8096"), APP_TLS, APP_TIMEOUT];
pub(super) const PLEX_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("32400"), APP_TLS, APP_TIMEOUT];
pub(super) const GITLAB_OPTIONS: &[OptionView] = &[
    app_scheme("https"),
    app_port("443"),
    APP_TLS,
    APP_TIMEOUT,
    text(
        "watched_projects",
        "Projects watched for CI health (optional)",
        "Comma-separated numeric project IDs. Their recent pipelines are read for failed and running counts; leave empty to skip pipeline health entirely.",
        "12,47",
        "",
    ),
    number(
        "pipeline_lookback_hours",
        "Failed pipeline window (hours)",
        "Pipelines older than this are not counted as recently failed. From 1 to 720.",
        "24",
        "24",
    ),
];
pub(super) const FORGEJO_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("3000"), APP_TLS, APP_TIMEOUT];

pub(super) const SONARR_APP_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("8989"), APP_TLS, APP_TIMEOUT];
pub(super) const RADARR_APP_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("7878"), APP_TLS, APP_TIMEOUT];
pub(super) const LIDARR_APP_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("8686"), APP_TLS, APP_TIMEOUT];
pub(super) const PROWLARR_APP_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("9696"), APP_TLS, APP_TIMEOUT];
pub(super) const QBITTORRENT_APP_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("8080"), APP_TLS, APP_TIMEOUT];
pub(super) const FRIGATE_APP_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("5000"), APP_TLS, APP_TIMEOUT];
pub(super) const KEYCLOAK_APP_OPTIONS: &[OptionView] = &[
    app_scheme("http"),
    app_port("8080"),
    text(
        "health_port",
        "Health port",
        "Port of the health checks. Keycloak 25 and later serve them on the management port (9000); type same for Keycloak 24 and earlier, which use the main port.",
        "9000",
        "9000",
    ),
    select(
        "health_scheme",
        "Health protocol",
        "Protocol of the management port. It is plain HTTP unless Keycloak is configured otherwise.",
        "http",
        &["http", "https"],
    ),
    text(
        "auth_realm",
        "Realm of the client",
        "The realm in which the service-account client was created.",
        "master",
        "master",
    ),
    text(
        "realms",
        "Realms to read (optional)",
        "Comma-separated realm names whose sessions and users are counted; leave empty for every realm, up to 20.",
        "homelab,staff",
        "",
    ),
    APP_TLS,
    APP_TIMEOUT,
];
pub(super) const AUTHENTIK_APP_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("9000"), APP_TLS, APP_TIMEOUT];
pub(super) const PAPRA_APP_OPTIONS: &[OptionView] =
    &[app_scheme("http"), app_port("1221"), APP_TLS, APP_TIMEOUT];
pub(super) const VAULTWARDEN_APP_OPTIONS: &[OptionView] =
    &[app_scheme("https"), app_port("443"), APP_TLS, APP_TIMEOUT];
pub(super) const MAILCOW_APP_OPTIONS: &[OptionView] =
    &[app_scheme("https"), app_port("443"), APP_TLS, APP_TIMEOUT];

pub(super) const ARR_KEY: CredentialView = CredentialView {
    kind: "api_token",
    label: "API key",
    help: "The API key of Settings → General → Security, sent in the X-Api-Key header.",
    fields: &[cred_secret("token", "API key", "Stored encrypted, never shown again.", "", true)],
};

pub(super) const QBITTORRENT_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "User name / password",
    help: "The WebUI account. DumbMonit opens a session and reuses its cookie for the probe.",
    fields: &[
        cred_text("username", "User name", "", "dumbmonit"),
        cred_secret("password", "Password", "", "", true),
    ],
};

pub(super) const QBITTORRENT_NO_AUTH: CredentialView = CredentialView {
    kind: "none",
    label: "No authentication (allowed address)",
    help: "For an address qBittorrent lets in without a login (whitelisted IP subnets).",
    fields: &[],
};

pub(super) const KEYCLOAK_NO_AUTH: CredentialView = CredentialView {
    kind: "none",
    label: "None (health checks only)",
    help: "Nothing is sent: the health checks are public. Realms, sessions and users need a client below.",
    fields: &[],
};

pub(super) const KEYCLOAK_CLIENT: CredentialView = CredentialView {
    kind: "username_password",
    label: "Client ID / secret",
    help: "A service-account client with read-only roles and the kc-admin-api client scope. DumbMonit asks for a token at each probe.",
    fields: &[
        cred_text("username", "Client ID", "", "dumbmonit"),
        cred_secret("password", "Client secret", "Stored encrypted, never shown again.", "", true),
    ],
};

pub(super) const AUTHENTIK_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "API token",
    help: "The key of an API token created for a service account, sent as \"Authorization: Bearer …\".",
    fields: &[cred_secret("token", "API token", "Stored encrypted, never shown again.", "", true)],
};

pub(super) const PAPRA_KEY: CredentialView = CredentialView {
    kind: "api_token",
    label: "API key",
    help: "An API key created in Papra's settings, sent as \"Authorization: Bearer …\".",
    fields: &[cred_secret("token", "API key", "Stored encrypted, never shown again.", "", true)],
};

pub(super) const VAULTWARDEN_NO_ADMIN: CredentialView = CredentialView {
    kind: "none",
    label: "No admin token (public checks only)",
    help: "Only /alive and /api/config are read, which Vaultwarden serves without any login.",
    fields: &[],
};

pub(super) const VAULTWARDEN_ADMIN: CredentialView = CredentialView {
    kind: "api_token",
    label: "Admin token",
    help: "The plain ADMIN_TOKEN of the admin page. It opens a session whose cookie DumbMonit keeps between probes; only counters are read.",
    fields: &[cred_secret(
        "token",
        "Admin token",
        "Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const MAILCOW_KEY: CredentialView = CredentialView {
    kind: "api_token",
    label: "Read-only API key",
    help: "The read-only key of System → Configuration → Access → API, sent in the X-API-Key header.",
    fields: &[cred_secret("token", "API key", "Stored encrypted, never shown again.", "", true)],
};

pub(super) const NEXTCLOUD_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "Monitoring token (NC-Token)",
    help: "The serverinfo token set with occ. It opens Nextcloud's statistics and nothing else.",
    fields: &[cred_secret("token", "Token", "Stored encrypted, never shown again.", "", true)],
};

pub(super) const IMMICH_KEY: CredentialView = CredentialView {
    kind: "api_token",
    label: "API key",
    help: "An Immich API key limited to the read permissions of the setup guide.",
    fields: &[cred_secret(
        "token",
        "API key",
        "Immich shows it once. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const PAPERLESS_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "API token (recommended)",
    help: "The token of the dedicated dumbmonit user, sent as \"Authorization: Token …\".",
    fields: &[cred_secret("token", "API token", "Stored encrypted, never shown again.", "", true)],
};

pub(super) const PAPERLESS_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "User name / password",
    help: "The dedicated dumbmonit user itself, sent as HTTP basic authentication.",
    fields: &[
        cred_text("username", "User name", "", "dumbmonit"),
        cred_secret("password", "Password", "", "", true),
    ],
};

pub(super) const JELLYFIN_KEY: CredentialView = CredentialView {
    kind: "api_token",
    label: "API key",
    help: "A key created in Jellyfin's dashboard, sent in the MediaBrowser authorization header.",
    fields: &[cred_secret("token", "API key", "Stored encrypted, never shown again.", "", true)],
};

pub(super) const PLEX_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "X-Plex-Token",
    help: "Needed by a server linked to a Plex account, unless this network is allowed without authentication.",
    fields: &[cred_secret("token", "Token", "Stored encrypted, never shown again.", "", true)],
};

pub(super) const PLEX_NO_TOKEN: CredentialView = CredentialView {
    kind: "none",
    label: "No token (allowed network)",
    help: "For a server that lists DumbMonit's address among the networks allowed without authentication.",
    fields: &[],
};

pub(super) const GITLAB_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "Personal access token",
    help: "An administrator's personal access token, sent as PRIVATE-TOKEN. A token without admin rights still works, but only the version is read.",
    fields: &[cred_secret(
        "token",
        "Personal access token",
        "Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const FORGEJO_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "Access token",
    help: "A site administrator's access token, sent as \"Authorization: token …\". A token without admin rights still works, but only the version and the account's own repositories are read.",
    fields: &[cred_secret(
        "token",
        "Access token",
        "Stored encrypted, never shown again.",
        "",
        true,
    )],
};

/// Mot de passe d'application Pi-hole : échangé contre une session, gardée
/// d'une interrogation à l'autre (`collectors/pihole/mod.rs`).
pub(super) const PIHOLE_APP_PASSWORD: CredentialView = CredentialView {
    kind: "api_token",
    label: "App password (recommended)",
    help: "Generated under Settings → Web interface / API in expert mode. It cannot change Pi-hole's settings.",
    fields: &[cred_secret(
        "token",
        "App password",
        "Pi-hole shows it once. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const PIHOLE_NO_PASSWORD: CredentialView = CredentialView {
    kind: "none",
    label: "No password",
    help: "Only for a Pi-hole whose web interface has no password at all.",
    fields: &[],
};

/// Options lues par `collectors/pihole/options.rs`.
pub(super) const PIHOLE_OPTIONS: &[OptionView] = &[
    select(
        "scheme",
        "Protocol",
        "Pi-hole serves plain HTTP on port 80 and HTTPS on port 443 with a certificate it signs itself.",
        "http",
        &["http", "https"],
    ),
    number("port", "Port", "Used if the address does not give a port.", "80", "80"),
    insecure_tls(
        "Pi-hole's own HTTPS certificate is self-signed: enable this to use HTTPS without installing your own certificate.",
    ),
    number(
        "request_timeout_seconds",
        "Timeout per request (seconds)",
        "Time allowed for each call, from 1 to 60.",
        "10",
        "10",
    ),
];

/// Compte AdGuard Home : un utilisateur de `AdGuardHome.yaml`, envoyé en
/// « basic » (`collectors/adguard/client.rs`).
pub(super) const ADGUARD_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "User name / password",
    help: "The dedicated user added to AdGuardHome.yaml, sent as HTTP basic authentication.",
    fields: &[
        cred_text("username", "User name", "", "dumbmonit"),
        cred_secret("password", "Password", "", "", true),
    ],
};

pub(super) const ADGUARD_NO_LOGIN: CredentialView = CredentialView {
    kind: "none",
    label: "No authentication",
    help: "Only for an AdGuard Home with no user at all in AdGuardHome.yaml.",
    fields: &[],
};

/// Options lues par `collectors/adguard/options.rs`.
pub(super) const ADGUARD_OPTIONS: &[OptionView] = &[
    select(
        "scheme",
        "Protocol",
        "AdGuard Home serves its web interface over plain HTTP until you give it a certificate in Settings → Encryption settings. HTTPS then.",
        "http",
        &["http", "https"],
    ),
    number(
        "port",
        "Port",
        "The web interface port chosen during setup. Used if the address does not give a port.",
        "80",
        "80",
    ),
    insecure_tls(
        "For a self-signed or private certificate: enable this if the connection is refused for that reason.",
    ),
    number(
        "request_timeout_seconds",
        "Timeout per request (seconds)",
        "Time allowed for each call, from 1 to 60.",
        "5",
        "5",
    ),
    boolean(
        "upstream_check",
        "Test upstream servers",
        "Asks AdGuard Home to test each upstream DNS server at every check, as its Test upstreams button does. An upstream that fails or stays silent for 4 seconds is reported down.",
        true,
    ),
];

/// NUT : `upsd` laisse lire toute machine qu'il accepte, sans compte
/// (`collectors/nut/mod.rs`).
pub(super) const NUT_NONE: CredentialView = CredentialView {
    kind: "none",
    label: "No authentication (recommended)",
    help: "upsd lets any host it accepts read the UPS variables, without an account.",
    fields: &[],
};

pub(super) const NUT_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "upsd user name / password",
    help: "Sent with USERNAME and PASSWORD before reading. upsd does not check them to read; set them only if your setup asks for them.",
    fields: &[
        cred_text(
            "username",
            "User name",
            "A user of upsd.users with no actions and no instcmds.",
            "dumbmonit",
        ),
        cred_secret("password", "Password", "", "", true),
    ],
};

/// Options lues par `collectors/nut/mod.rs`.
pub(super) const NUT_OPTIONS: &[OptionView] = &[
    number("port", "Port", "The upsd port, used if the address does not give one.", "3493", "3493"),
    text(
        "ups",
        "UPS",
        "Comma-separated UPS names to watch, as upsd knows them (\"ups\" on a Synology NAS, \"qnapups\" on a QNAP). Empty watches every UPS the server publishes.",
        "ups",
        "",
    ),
    number(
        "request_timeout_seconds",
        "Timeout per exchange (seconds)",
        "Time allowed for the connection and each answer, from 1 to 60.",
        "10",
        "10",
    ),
];

/// Compte RouterOS dans un groupe aux politiques `read,api,rest-api`
/// (`collectors/mikrotik/client.rs`).
pub(super) const MIKROTIK_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "Read-only user",
    help: "The dumbmonit user created above, in the dumbmonit-read group. Sent as HTTP basic authentication.",
    fields: &[
        cred_text("username", "User name", "", "dumbmonit"),
        cred_secret("password", "Password", "Stored encrypted, never shown again.", "", true),
    ],
};

/// Options lues par `collectors/mikrotik/options.rs`.
pub(super) const MIKROTIK_OPTIONS: &[OptionView] = &[
    select(
        "scheme",
        "Protocol",
        "HTTPS is the www-ssl service. HTTP (the www service) also works, but the password then crosses the network in clear.",
        "https",
        &["https", "http"],
    ),
    number(
        "port",
        "Port",
        "Used if the address does not give a port. Empty: 443 for HTTPS, 80 for HTTP.",
        "443",
        "",
    ),
    insecure_tls(
        "A certificate made on the router itself cannot be verified: enable this if the connection is refused for that reason.",
    ),
    number(
        "request_timeout_seconds",
        "Timeout per request (seconds)",
        "Time allowed for each call, from 1 to 60.",
        "10",
        "10",
    ),
    boolean(
        "interfaces",
        "Read the interfaces",
        "State, traffic, errors and link losses of every enabled interface. Dynamic ones (PPPoE, L2TP clients) are left out.",
        true,
    ),
    number(
        "max_interfaces",
        "Interfaces read at most",
        "Beyond this, the next interfaces are counted but not read. From 1 to 1000.",
        "64",
        "64",
    ),
];

/// Clé d'API UniFi, envoyée en `X-API-KEY` (`collectors/unifi`).
pub(super) const UNIFI_KEY: CredentialView = CredentialView {
    kind: "api_token",
    label: "API key (UniFi OS console)",
    help: "Reads devices, their load and the client count through the official Integration API. Created in Settings → Control Plane → Integrations.",
    fields: &[cred_secret(
        "token",
        "API key",
        "UniFi shows it once. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

pub(super) const UNIFI_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "View Only account",
    help: "A local account with the View Only role. Also reads the WAN, Internet and alarm status, and works on a self-hosted server.",
    fields: &[
        cred_text("username", "User name", "", "dumbmonit"),
        cred_secret("password", "Password", "", "", true),
    ],
};

/// Options lues par `collectors/unifi/mod.rs` et `collectors/api_options.rs`.
pub(super) const UNIFI_OPTIONS: &[OptionView] = &[
    text(
        "site",
        "Site",
        "The site's short name, shown in the address bar after /manage/ or /network/. A key also accepts the site's displayed name.",
        "default",
        "default",
    ),
    number(
        "port",
        "Port",
        "443 on a UniFi OS console, 8443 on a self-hosted UniFi Network Server. Used if the address does not give a port.",
        "443",
        "443",
    ),
    insecure_tls(
        "Consoles and self-hosted servers use a self-signed certificate by default: enable this unless you installed your own.",
    ),
    number(
        "request_timeout_seconds",
        "Timeout per request (seconds)",
        "Time allowed for each call, from 1 to 120.",
        "15",
        "15",
    ),
    text(
        "watched_clients",
        "Clients to watch besides the marked ones",
        "Comma-separated MAC addresses or client names, or all. Clients named, reserved on a fixed IP, or noted in UniFi are always watched; most phones and guest devices are not, to avoid an alert for every visitor.",
        "all",
        "",
    ),
    DEVICE_STALE_DAYS,
];

/// Jeton longue durée de Home Assistant, envoyé en `Authorization: Bearer`.
pub(super) const HOMEASSISTANT_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "Long-lived access token",
    help: "Created in the dedicated user's profile, under Security.",
    fields: &[cred_secret(
        "token",
        "Access token",
        "Home Assistant shows it once. Stored encrypted, never shown again.",
        "",
        true,
    )],
};

/// Options lues par `collectors/homeassistant/mod.rs`.
pub(super) const HOMEASSISTANT_OPTIONS: &[OptionView] = &[
    select(
        "scheme",
        "Protocol",
        "Home Assistant listens over plain HTTP unless TLS is configured or it sits behind a reverse proxy. HTTPS then.",
        "http",
        &["http", "https"],
    ),
    number("port", "Port", "Used if the address does not give a port.", "8123", "8123"),
    insecure_tls(
        "For a self-signed certificate or a private certificate authority: enable this if the connection is refused for that reason.",
    ),
    number(
        "request_timeout_seconds",
        "Timeout per request (seconds)",
        "Time allowed for each call, from 1 to 120.",
        "10",
        "10",
    ),
    number(
        "battery_threshold",
        "Low battery threshold (%)",
        "A battery sensor below this level counts as low. Battery binary sensors count as low when they are on.",
        "20",
        "20",
    ),
    text(
        "exclude_domains",
        "Domains left out",
        "Comma-separated entity domains whose unavailable or unknown entities are not counted, for example device_tracker or media_player for devices that are often off.",
        "device_tracker, media_player",
        "",
    ),
    boolean(
        "repairs",
        "Read the repairs",
        "Reads the open repairs over the WebSocket API. The REST API does not list them.",
        true,
    ),
    DEVICE_STALE_DAYS,
];

pub(super) const VSPHERE_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "User name / password",
    help: "The dedicated user with the Read-only role. On a vCenter, include the domain.",
    fields: &[
        cred_text("username", "User name", "", "dumbmonit@vsphere.local"),
        cred_secret("password", "Password", "", "", true),
    ],
};

/// Options lues par `collectors/vsphere/mod.rs`.
pub(super) const VSPHERE_OPTIONS: &[OptionView] = &[
    number("port", "Port", "Used if the address does not give a port.", "443", "443"),
    insecure_tls(
        "vCenter and ESXi use a self-signed certificate by default: enable this unless your DumbMonit host trusts their certificate authority.",
    ),
    number(
        "request_timeout_seconds",
        "Timeout per request (seconds)",
        "Time allowed for each call, from 1 to 120. A large inventory takes longer.",
        "20",
        "20",
    ),
    boolean(
        "alarms",
        "Read the triggered alarms",
        "Reads the alarms vCenter or the host raised, with the object each one is about.",
        true,
    ),
];
