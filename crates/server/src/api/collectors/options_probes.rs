//! Réglages des sondes de disponibilité et des moniteurs web (HTTP, scénario,
//! TCP, DNS, ping, TLS, bases de données, messagerie, push, netconfig,
//! changements et santé d'un site).

use super::*;

/// Délai propre à une sonde de disponibilité, commun aux cinq (`uptime/tags.rs`).
///
/// Le plafond de 60 s est celui du collecteur : au-delà, le délai global du
/// planificateur interromprait la sonde avant qu'elle n'ait pu enregistrer
/// l'indisponibilité.
pub(super) const PROBE_TIMEOUT: OptionView = number(
    "timeout_seconds",
    "Timeout (seconds)",
    "Time after which the service is reported down if it has not answered. Between 1 and 60.",
    "5",
    "5",
);

/// Accepter un certificat que l'on ne peut pas vérifier (auto-signé, autorité
/// privée). Même clé et même sens pour HTTP, TLS, Proxmox et Synology.
pub(super) const fn insecure_tls(help: &'static str) -> OptionView {
    boolean("insecure_tls", "Accept an unverifiable certificate", help, false)
}

/// Lever le garde-fou d'adresses des sondes (`collectors/uptime/guard.rs`) : la
/// boucle locale et le lien local sont refusés par défaut, parce qu'ils ne sont
/// joignables que depuis l'hôte de supervision lui-même.
pub(super) const ALLOW_PRIVATE_TARGETS: OptionView = boolean(
    "allow_private_targets",
    "Allow loopback and link-local targets",
    "By default the check refuses addresses only the DumbMonit host itself can reach (127.0.0.1, ::1, 169.254.x.x). Enable this to monitor a service running on the DumbMonit host. Private LAN addresses (10.x, 192.168.x) are always allowed.",
    false,
);

/// Options lues par `collectors/uptime/http/options.rs`.
pub(super) const HTTP_OPTIONS: &[OptionView] = &[
    select(
        "method",
        "HTTP method",
        "GET fits almost every case. HEAD avoids downloading the page when only the status code matters.",
        "GET",
        &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
    ),
    text(
        "accepted_status",
        "Accepted status codes",
        "Codes or ranges considered normal, separated by commas. Any other code counts as down.",
        "200-299,301,404",
        "200-299",
    ),
    text(
        "keyword",
        "Expected keyword",
        "Text that must appear in the page. If it is missing, the service is reported down even though the page answers.",
        "Welcome",
        "",
    ),
    boolean(
        "keyword_absent",
        "Keyword must be absent",
        "Inverts the check: the presence of the keyword signals a failure. Useful for an error page that answers 200.",
        false,
    ),
    boolean(
        "keyword_case_sensitive",
        "Match keyword case",
        "By default, upper and lower case are treated the same.",
        false,
    ),
    text(
        "json_path",
        "JSON path to check",
        "For a JSON response: path of the value to check. Fill it in together with the expected value, never one without the other.",
        "$.status",
        "",
    ),
    text(
        "json_expect",
        "Expected JSON value",
        "Value the JSON path above must have, for example \"ok\" or \"true\".",
        "ok",
        "",
    ),
    text(
        "headers",
        "Extra headers",
        "One or more \"Name: value\" headers, separated by \"|\". Stored unencrypted and hidden from viewers only: put tokens and passwords in the credential instead.",
        "Accept: application/json | X-Origin: dumbmonit",
        "",
    ),
    text(
        "body",
        "Request body",
        "Content sent with the request, for methods that expect one (POST, PUT…).",
        "",
        "",
    ),
    boolean(
        "follow_redirects",
        "Follow redirects",
        "Disable to monitor the redirect itself, for example a 301 to HTTPS.",
        true,
    ),
    number(
        "max_redirects",
        "Maximum redirects followed",
        "Between 1 and 20. No effect if redirects are not followed.",
        "10",
        "10",
    ),
    insecure_tls(
        "The check no longer fails on a self-signed certificate or one issued by a private authority.",
    ),
    ALLOW_PRIVATE_TARGETS,
    boolean(
        "check_certificate",
        "Read the certificate",
        "Over HTTPS, also records the certificate expiry date so you can be warned before it expires.",
        true,
    ),
    number(
        "max_body_bytes",
        "Maximum bytes read",
        "Beyond this, the rest of the page is not downloaded. The keyword and JSON path are only searched in this part.",
        "524288",
        "524288",
    ),
    text(
        "user_agent",
        "Announced identity (User-Agent)",
        "Name the check gives to the server. Change it if the server filters robots.",
        "DumbMonit/1.0",
        concat!("DumbMonit/", env!("CARGO_PKG_VERSION")),
    ),
    PROBE_TIMEOUT,
];

/// Les étapes d'un scénario HTTP : un tableau JSON que l'interface saisit dans
/// son éditeur d'étapes (`input: steps`), lu par
/// `collectors/uptime/http/scenario/model.rs`.
pub(super) const HTTP_STEPS_LIST: OptionView = OptionView {
    key: "steps",
    label: "Steps",
    help: "The requests played in order, from 1 to 10. Each step checks the status code and, optionally, a text or a JSON value; it can keep a value from the response (JSON path, header or regular expression) under a name that later steps write as {{name}}. Cookies set by a response are sent with the following requests to the same site.",
    placeholder: "",
    default: "",
    required: true,
    input: "steps",
    choices: &[],
    group: "",
    items: no_items,
    columns: no_columns,
};

/// Options lues par `collectors/uptime/http/scenario/mod.rs`.
pub(super) const HTTP_STEPS_OPTIONS: &[OptionView] = &[
    HTTP_STEPS_LIST,
    boolean(
        "follow_redirects",
        "Follow redirects",
        "Disable to check the redirect a step receives instead of the page it leads to.",
        true,
    ),
    number(
        "max_redirects",
        "Maximum redirects per step",
        "Between 1 and 20. No effect if redirects are not followed.",
        "10",
        "10",
    ),
    insecure_tls(
        "The steps no longer fail on a self-signed certificate or one issued by a private authority.",
    ),
    ALLOW_PRIVATE_TARGETS,
    boolean(
        "check_certificate",
        "Read the certificate",
        "Over HTTPS, also records the certificate expiry date of the address so you can be warned before it expires.",
        true,
    ),
    number(
        "max_body_bytes",
        "Maximum bytes read per step",
        "Beyond this, the rest of a response is not downloaded. Checks and extractions only see this part.",
        "524288",
        "524288",
    ),
    text(
        "user_agent",
        "Announced identity (User-Agent)",
        "Name the steps give to the server. Change it if the server filters robots.",
        "DumbMonit/1.0",
        concat!("DumbMonit/", env!("CARGO_PKG_VERSION")),
    ),
    number(
        "timeout_seconds",
        "Timeout (seconds)",
        "Time allowed for the whole scenario, every step included, before it is reported down. Between 1 and 60.",
        "5",
        "5",
    ),
];

/// Options lues par `collectors/uptime/tcp.rs`.
pub(super) const TCP_OPTIONS: &[OptionView] = &[
    number(
        "port",
        "Port",
        "Port to open, if the address does not already give it as \"host:port\". One of the two is required.",
        "22",
        "",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/dns/options.rs`.
pub(super) const DNS_OPTIONS: &[OptionView] = &[
    select(
        "record_type",
        "Record type",
        "A for an IPv4 address, AAAA for IPv6, MX for mail, CNAME for an alias…",
        "A",
        &["A", "AAAA", "CNAME", "MX", "TXT", "NS", "SOA", "SRV", "PTR", "CAA"],
    ),
    text(
        "resolver",
        "DNS server to query",
        "IP address of a resolver, port optional. Empty: the system resolver. Handy to check your own DNS server.",
        "1.1.1.1 or 10.0.0.1:5353",
        "",
    ),
    text(
        "expect",
        "Expected values",
        "Values that must all appear in the answer, separated by commas. Empty: only the resolution is checked.",
        "93.184.216.34",
        "",
    ),
    select(
        "expect_mode",
        "Comparison",
        "\"contains\": each expected value must appear somewhere in the answer. \"exact\": the answer must hold those values and nothing else, so a record added beside the right one fails the check.",
        "contains",
        &["contains", "exact"],
    ),
    text(
        "forbid",
        "Forbidden values",
        "Values that must never appear in the answer, separated by commas: a former host's address, an expired validation record. Checked before the expected ones.",
        "198.51.100.7",
        "",
    ),
    boolean(
        "alert_on_change",
        "Alert when the answer changes",
        "Records a fingerprint of the sorted answer and alerts whenever it changes, without having to write the expected values. Leave off for names whose answer rotates between servers (large round-robin pools, geographic DNS).",
        false,
    ),
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/ping/options.rs`.
pub(super) const PING_OPTIONS: &[OptionView] = &[
    number("count", "Number of echoes", "Packets sent on each poll, from 1 to 20.", "4", "4"),
    number(
        "packet_timeout_ms",
        "Wait per packet (ms)",
        "Time allowed for each reply, from 50 to 10,000 ms.",
        "1000",
        "1000",
    ),
    number(
        "interval_ms",
        "Gap between two echoes (ms)",
        "Pause between two echoes, from 0 to 5,000 ms.",
        "100",
        "100",
    ),
    number(
        "payload_bytes",
        "Payload size (bytes)",
        "Data carried by each echo, from 0 to 1,400 bytes.",
        "56",
        "56",
    ),
    select(
        "ip_version",
        "IP version",
        "\"auto\" takes the first resolved address; force 4 or 6 if the host has both and one does not answer.",
        "auto",
        &["auto", "4", "6"],
    ),
    number(
        "loss_threshold_percent",
        "Tolerated loss (%)",
        "Above this percentage of lost packets, the host is reported down. 100: only a total loss counts, partial loss stays visible in the charts.",
        "100",
        "100",
    ),
    number(
        "timeout_seconds",
        "Timeout (seconds)",
        "Total budget for the poll, from 1 to 60. Refused if it does not cover \"number of echoes × wait per packet\".",
        "5",
        "5",
    ),
];

/// Options lues par `collectors/uptime/tls/options.rs`.
pub(super) const TLS_OPTIONS: &[OptionView] = &[
    text(
        "server_name",
        "Server name (SNI)",
        "Domain name announced to the server and checked in the certificate. Set it when the address is an IP behind a reverse proxy.",
        "www.example.com",
        "",
    ),
    insecure_tls(
        "An unverifiable chain no longer counts as a failure: the expiry date is still recorded.",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Nom annoncé en SNI, commun aux sondes applicatives chiffrées.
pub(super) const fn server_name_option(help: &'static str) -> OptionView {
    text("server_name", "Server name (SNI)", help, "mail.example.com", "")
}

/// Options lues par `collectors/uptime/smtp/options.rs`.
pub(super) const SMTP_OPTIONS: &[OptionView] = &[
    select(
        "security",
        "Encryption",
        "STARTTLS starts in clear text and upgrades (port 587), TLS encrypts from the first byte (port 465), None never encrypts (port 25) and only suits a relay on your own network.",
        "starttls",
        &["starttls", "tls", "none"],
    ),
    number(
        "port",
        "Port",
        "Used if the address does not give one. Empty: 587 with STARTTLS, 465 with TLS, 25 without encryption.",
        "587",
        "",
    ),
    text(
        "helo_name",
        "Name announced (EHLO)",
        "Name the check gives when it introduces itself. A strict relay refuses a name it cannot resolve.",
        "monit.example.com",
        "dumbmonit",
    ),
    text(
        "expect_capability",
        "Expected extension",
        "Extension the server must advertise in its EHLO answer, for example STARTTLS or AUTH. Empty: no expectation.",
        "STARTTLS",
        "",
    ),
    server_name_option(
        "Domain name announced to the server and checked in the certificate. Set it when the address is an IP.",
    ),
    insecure_tls(
        "An unverifiable chain no longer counts as a failure: the expiry date is still recorded.",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/sql/options.rs`, moteur PostgreSQL.
pub(super) const POSTGRES_OPTIONS: &[OptionView] = &[
    number("port", "Port", "Used if the address does not give one.", "5432", "5432"),
    text(
        "database",
        "Database",
        "Database opened on connection. The monitoring account must be allowed to connect to it.",
        "postgres",
        "postgres",
    ),
    text(
        "query",
        "Query",
        "Run on every check. The default reads no table, so the account needs no privilege beyond connecting. A query returning one number turns it into a chart.",
        "SELECT count(*) FROM jobs WHERE failed",
        "SELECT 1",
    ),
    text(
        "expect",
        "Expected value",
        "Value the first column of the first row must hold. Empty: only the query succeeding is checked.",
        "1",
        "",
    ),
    select(
        "sslmode",
        "Encryption",
        "\"prefer\" encrypts when the server offers it, \"require\" refuses to connect without it, \"verify-full\" also checks the certificate and the name, \"disable\" never encrypts.",
        "prefer",
        &["prefer", "require", "verify-full", "disable"],
    ),
    boolean(
        "server_stats",
        "Server health metrics",
        "Also reads connections, long transactions, locks, replication, transaction ID age, dead rows and WAL archiving from the server's statistics views, on the same connection. A role without pg_monitor sees fewer of them; nothing here can make the check fail.",
        true,
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/sql/options.rs`, moteur MySQL/MariaDB.
pub(super) const MYSQL_OPTIONS: &[OptionView] = &[
    number("port", "Port", "Used if the address does not give one.", "3306", "3306"),
    text(
        "database",
        "Database",
        "Database opened on connection. Empty: none, which is enough for the default query.",
        "monitoring",
        "",
    ),
    text(
        "query",
        "Query",
        "Run on every check. The default reads no table, so the account needs no privilege beyond connecting. A query returning one number turns it into a chart.",
        "SELECT count(*) FROM jobs WHERE failed",
        "SELECT 1",
    ),
    text(
        "expect",
        "Expected value",
        "Value the first column of the first row must hold. Empty: only the query succeeding is checked.",
        "1",
        "",
    ),
    select(
        "sslmode",
        "Encryption",
        "\"prefer\" encrypts when the server offers it, \"require\" refuses to connect without it, \"verify-full\" also checks the certificate and the name, \"disable\" never encrypts.",
        "prefer",
        &["prefer", "require", "verify-full", "disable"],
    ),
    boolean(
        "server_stats",
        "Server health metrics",
        "Also reads connections, queries per second, slow queries, the InnoDB buffer pool hit ratio, replication and its lag, schema sizes and long-running queries, on the same connection. An account without PROCESS or REPLICATION CLIENT sees fewer of them; nothing here can make the check fail.",
        true,
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/mqtt/options.rs`.
pub(super) const MQTT_OPTIONS: &[OptionView] = &[
    boolean(
        "tls",
        "Encrypted connection",
        "Encrypts the session from the first byte, as brokers do on port 8883.",
        false,
    ),
    number(
        "port",
        "Port",
        "Used if the address does not give one. Empty: 1883 in clear text, 8883 encrypted.",
        "1883",
        "",
    ),
    text(
        "topic",
        "Topic",
        "Topic the check subscribes to. Empty: it only connects, which already tells you the broker is alive and accepts your account.",
        "home/living-room/temperature",
        "",
    ),
    boolean(
        "expect_message",
        "Expect a retained message",
        "A message must arrive on that topic before the timeout. Only retained messages arrive right away: a topic that is merely published to from time to time will look silent.",
        false,
    ),
    text(
        "expect",
        "Expected content",
        "Text that message must contain. Filling it in implies expecting a message.",
        "online",
        "",
    ),
    text(
        "client_id",
        "Client identifier",
        "Name announced to the broker. The default derives from the monitor, so two checks never disconnect each other.",
        "dumbmonit-3",
        "",
    ),
    server_name_option(
        "Domain name announced to the broker and checked in the certificate. Set it when the address is an IP.",
    ),
    insecure_tls(
        "An unverifiable chain no longer counts as a failure: the expiry date is still recorded.",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/websocket/options.rs`.
pub(super) const WEBSOCKET_OPTIONS: &[OptionView] = &[
    text(
        "path",
        "Path",
        "Path of the opening request. Taken from the address when it carries one.",
        "/api/websocket",
        "",
    ),
    number(
        "port",
        "Port",
        "Used if the address does not give one. Empty: 443 for wss, 80 for ws.",
        "8123",
        "",
    ),
    text(
        "send",
        "Frame to send",
        "Text frame sent once the connection is open. Empty: nothing is sent.",
        "{\"type\":\"ping\"}",
        "",
    ),
    text(
        "expect",
        "Expected content",
        "Text a received frame must contain. Empty and with nothing to send, only the opening handshake is checked.",
        "auth_required",
        "",
    ),
    text(
        "subprotocol",
        "Subprotocol",
        "Value of Sec-WebSocket-Protocol. The check fails if the server picks a different one.",
        "json",
        "",
    ),
    text(
        "origin",
        "Origin",
        "Value of the Origin header, which some servers require before upgrading.",
        "https://home.example.com",
        "",
    ),
    server_name_option(
        "Domain name announced to the server and checked in the certificate. Set it when the address is an IP.",
    ),
    insecure_tls(
        "An unverifiable chain no longer counts as a failure: the expiry date is still recorded.",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/ntp/options.rs`.
pub(super) const NTP_OPTIONS: &[OptionView] = &[
    number("port", "Port", "UDP port of the NTP server.", "123", "123"),
    number(
        "offset_threshold_ms",
        "Tolerated offset (ms)",
        "Above this clock offset, the server is reported down. An unsynchronized server (stratum 16) always fails, whatever this value.",
        "100",
        "100",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/udp/options.rs`.
pub(super) const UDP_OPTIONS: &[OptionView] = &[
    number(
        "port",
        "Port",
        "Used if the address does not give one. There is no default: UDP has no standard port to fall back on.",
        "5060",
        "",
    ),
    text(
        "payload",
        "Datagram to send",
        "Sent as one datagram, 1400 bytes at most. In text, \\n \\r \\t \\0 \\\\ and \\xNN are read as escapes. Empty: an empty datagram.",
        "PING",
        "",
    ),
    select(
        "payload_encoding",
        "Datagram encoding",
        "Text sends the characters as typed, Hex reads pairs of hexadecimal digits (spaces allowed).",
        "text",
        &["text", "hex"],
    ),
    text(
        "expect_response",
        "Expected reply",
        "Text the reply must contain. Empty: the check succeeds unless the host answers that the port is closed, and silence counts as success.",
        "PONG",
        "",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/ssh/options.rs`.
pub(super) const SSH_OPTIONS: &[OptionView] = &[
    number("port", "Port", "Used if the address does not give one.", "22", "22"),
    text(
        "expected_host_key",
        "Expected host key",
        "SHA256 fingerprint of the server's host key, as printed by ssh-keygen -lf. If the key differs, the server is reported down with the reason host_key. Empty: the key is not checked.",
        "SHA256:nThbg6kXUpJWGl7E1IGOCspRomTxdCARLviKw6E5SY8",
        "",
    ),
    boolean(
        "alert_on_change",
        "Alert when the host key changes",
        "Records a fingerprint of the host key on every check, so that a rule can fire when it changes. Implied by an expected host key.",
        false,
    ),
    select(
        "host_key_type",
        "Host key type",
        "A server may hold several host keys. Auto reads the first one it offers in this order: ed25519, ecdsa, rsa. Pick the family of the fingerprint you expect.",
        "auto",
        &["auto", "ed25519", "ecdsa", "rsa"],
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Chiffrement des sondes de boîte aux lettres (`collectors/uptime/mailbox.rs`).
pub(super) const fn mailbox_security(help: &'static str) -> OptionView {
    select("security", "Encryption", help, "tls", &["tls", "starttls", "none"])
}

/// Options lues par `collectors/uptime/imap/options.rs`.
pub(super) const IMAP_OPTIONS: &[OptionView] = &[
    mailbox_security(
        "TLS encrypts from the first byte (port 993), STARTTLS starts in clear text and upgrades (port 143), None never encrypts (port 143) and sends the password in clear text: only for your own network.",
    ),
    number(
        "port",
        "Port",
        "Used if the address does not give one. Empty: 993 with TLS, 143 with STARTTLS or without encryption.",
        "993",
        "",
    ),
    server_name_option(
        "Domain name announced to the server and checked in the certificate. Set it when the address is an IP.",
    ),
    insecure_tls(
        "An unverifiable chain no longer counts as a failure: the expiry date is still recorded.",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/pop3/options.rs`.
pub(super) const POP3_OPTIONS: &[OptionView] = &[
    mailbox_security(
        "TLS encrypts from the first byte (port 995), STARTTLS starts in clear text and upgrades with STLS (port 110), None never encrypts (port 110) and sends the password in clear text: only for your own network.",
    ),
    number(
        "port",
        "Port",
        "Used if the address does not give one. Empty: 995 with TLS, 110 with STARTTLS or without encryption.",
        "995",
        "",
    ),
    server_name_option(
        "Domain name announced to the server and checked in the certificate. Set it when the address is an IP.",
    ),
    insecure_tls(
        "An unverifiable chain no longer counts as a failure: the expiry date is still recorded.",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Identifiants d'un relais de messagerie ou d'un courtier MQTT.
pub(super) const APP_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "Username / password",
    help: "Sent once the connection is encrypted, never before.",
    fields: &[
        cred_text("username", "User name", "", "monitoring"),
        cred_secret("password", "Password", "Stored encrypted, never shown again.", "", true),
    ],
};

/// Identifiants d'un compte de base de données.
pub(super) const SQL_LOGIN: CredentialView = CredentialView {
    kind: "username_password",
    label: "Username / password",
    help: "A read-only account created for monitoring, never an application one.",
    fields: &[
        cred_text("username", "User name", "", "dumbmonit"),
        cred_secret("password", "Password", "Stored encrypted, never shown again.", "", true),
    ],
};

/// Options lues par `collectors/uptime/snmp_oid/options.rs`.
pub(super) const SNMP_OID_OPTIONS: &[OptionView] = &[
    text(
        "oid",
        "Object (OID)",
        "The SNMP object to read, in numeric form. Add the instance suffix, usually .0 for a single value.",
        "1.3.6.1.2.1.1.3.0",
        "",
    ),
    number("port", "Port", "Used if the address does not give one.", "161", "161"),
    text(
        "min",
        "Minimum value",
        "A numeric value below this fails. Empty: no lower bound.",
        "10",
        "",
    ),
    text(
        "max",
        "Maximum value",
        "A numeric value above this fails. Empty: no upper bound.",
        "75",
        "",
    ),
    text(
        "expect",
        "Expected text",
        "Text the value must contain, whatever its case. Empty: any value is accepted.",
        "online",
        "",
    ),
    select(
        "snmp_version",
        "SNMP version",
        "Version used with a community. Pick 1 only for an old agent that ignores version 2c; SNMP v3 users ignore it.",
        "2c",
        &["2c", "1"],
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Identifiants d'une sonde RADIUS : le secret du serveur et un compte de test.
pub(super) const RADIUS_LOGIN: CredentialView = CredentialView {
    kind: "radius",
    label: "Shared secret and test account",
    help: "The secret defined for this host on the RADIUS server, and an account created for monitoring.",
    fields: &[
        cred_secret(
            "shared_secret",
            "Shared secret",
            "As configured for this client on the RADIUS server. Stored encrypted, never shown again.",
            "",
            true,
        ),
        cred_text(
            "username",
            "Test account",
            "An account without a second factor, used only to check that the server authenticates.",
            "dumbmonit",
        ),
        cred_secret(
            "password",
            "Test account password",
            "Stored encrypted, never shown again.",
            "",
            true,
        ),
    ],
};

/// Options lues par `collectors/uptime/radius/options.rs`.
pub(super) const RADIUS_OPTIONS: &[OptionView] = &[
    number("port", "Port", "UDP port of the authentication service.", "1812", "1812"),
    text(
        "nas_identifier",
        "NAS identifier",
        "Value of the NAS-Identifier attribute sent with the request. Change it if the server filters requests by NAS.",
        "dumbmonit",
        "dumbmonit",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/uptime/grpc/options.rs`.
pub(super) const GRPC_OPTIONS: &[OptionView] = &[
    text(
        "service",
        "Service",
        "Name of the service whose health is asked, for example my.package.MyService. Empty: the health of the server as a whole.",
        "my.package.MyService",
        "",
    ),
    boolean(
        "tls",
        "Encrypted connection (TLS)",
        "Connect with TLS and HTTP/2 (ALPN h2). Unticked, the check speaks HTTP/2 in clear text (h2c), as most services inside a network do.",
        false,
    ),
    number(
        "port",
        "Port",
        "Used if the address does not give one. Empty: 50051, or 443 with TLS.",
        "50051",
        "",
    ),
    server_name_option(
        "Domain name announced to the server and checked in the certificate. Set it when the address is an IP.",
    ),
    insecure_tls(
        "An unverifiable chain no longer counts as a failure: the expiry date is still recorded.",
    ),
    ALLOW_PRIVATE_TARGETS,
    PROBE_TIMEOUT,
];

/// Options lues par `collectors/push/mod.rs` (`Settings::from_target`).
pub(super) const PUSH_OPTIONS: &[OptionView] = &[
    text(
        "expected_interval",
        "Expected interval",
        "How often the job is supposed to call in: 30m, 1h, 6h, 24h, 7d (or a number of seconds). A missed call is declared once this interval plus the grace period has passed.",
        "24h",
        "24h",
    ),
    text(
        "grace",
        "Grace period",
        "Extra time tolerated after the expected interval before the heartbeat counts as missed: a percentage of the interval (10%) or a fixed duration (15m). Never less than one minute.",
        "10%",
        "10%",
    ),
    text(
        "schedule",
        "Cron schedule",
        "Instead of the interval above: when the job is supposed to start, as a 5-field cron expression (minute hour day-of-month month day-of-week) or a shortcut such as @daily. Lists, ranges, steps and names are accepted: 30 2 * * *, */15 * * * *, 0 4 * * sun. Empty: use the interval.",
        "30 2 * * *",
        "",
    ),
    text(
        "timezone",
        "Time zone",
        "IANA time zone in which the cron schedule is read, for example Europe/Paris. It follows daylight saving time.",
        "Europe/Paris",
        "UTC",
    ),
    text(
        "max_duration",
        "Maximum duration",
        "A run that lasts longer than this raises the \"job too long\" alert: 30m, 2h. Needs a start call at the beginning of the job. Empty: a run still open after one period plus the grace period counts as stuck.",
        "2h",
        "",
    ),
];

/// Compte SSH d'une sauvegarde de configuration (`netconfig`).
pub(super) const SSH_LOGIN: CredentialView = CredentialView {
    kind: "ssh_login",
    label: "SSH login",
    help: "A read-only account is enough on most devices. Stored encrypted, never shown again.",
    fields: &[
        cred_text("username", "User name", "", "backup"),
        cred_secret("password", "Password", "", "", true),
        cred_secret(
            "enable_secret",
            "Enable password",
            "Only for a profile that enters privileged mode (Cisco IOS, Aruba) with another password than the login's. Empty: the login password, or the saved one.",
            "",
            false,
        ),
    ],
};

/// Clé et secret d'API (OPNsense) d'une sauvegarde de configuration.
pub(super) const NETCONFIG_API_KEY: CredentialView = CredentialView {
    kind: "username_password",
    label: "API key and secret",
    help: "OPNsense API profile: the key and secret of an API user allowed to download backups.",
    fields: &[
        cred_text("username", "API key", "", ""),
        cred_secret("password", "API secret", "", "", true),
    ],
};

/// Clé d'API (PAN-OS) d'une sauvegarde de configuration.
pub(super) const NETCONFIG_API_TOKEN: CredentialView = CredentialView {
    kind: "api_token",
    label: "API key",
    help: "PAN-OS API profile: the API key of a read-only role, sent in the X-PAN-KEY header.",
    fields: &[cred_secret("token", "API key", "Stored encrypted, never shown again.", "", true)],
};

/// Options lues par `netconfig/mod.rs` (`Schedule`) et
/// `dumbmonit_collectors::netconfig` (`FetchOptions`).
pub(super) const NETCONFIG_OPTIONS: &[OptionView] = &[
    OptionView {
        required: true,
        ..select(
            "profile",
            "Device model",
            "How to talk to the device and which commands to send. Only the commands of the chosen profile are ever sent.",
            "cisco_ios",
            dumbmonit_collectors::netconfig::PROFILE_IDS,
        )
    },
    number(
        "fetch_interval_hours",
        "Back up every (hours)",
        "How often the configuration is read, from 1 to 168. A failed read is retried after 15 minutes.",
        "24",
        "24",
    ),
    number(
        "keep_versions",
        "Versions kept",
        "Older versions beyond this number are deleted, from 1 to 1000. The latest one is always kept.",
        "100",
        "100",
    ),
    number(
        "keep_days",
        "Keep versions for (days)",
        "Versions not seen for this many days are deleted, from 0 (no age limit) to 3650. The latest one is always kept.",
        "365",
        "365",
    ),
    number(
        "fetch_timeout_seconds",
        "Timeout per step (seconds)",
        "Time allowed for the connection, each prompt and each command, from 10 to 110.",
        "60",
        "60",
    ),
    insecure_tls(
        "API profiles only (OPNsense, PAN-OS): for a device of your own behind a self-signed certificate.",
    ),
];

/// Options lues par `webchange/options.rs` (`Options::from_target`).
pub(super) const WEBCHANGE_OPTIONS: &[OptionView] = &[
    select(
        "scope",
        "Scope",
        "Page: only the address above. Site: every page under the same folder, found through the site's sitemap or by following its links.",
        "page",
        &["page", "site"],
    ),
    number(
        "max_pages",
        "Max pages",
        "Site scope only: pages read at most on each check, from 1 to 200.",
        "25",
        "25",
    ),
    text(
        "path_prefix",
        "Path prefix",
        "Site scope only: follow only the pages whose path starts with this, for example /docs/. Empty: the folder of the address above.",
        "/docs/",
        "",
    ),
    text(
        "ignore",
        "Ignore lines",
        "Regular expressions, one per line or joined with |: the text lines they match are left out of the comparison. For clocks, counters and \"last updated\" lines.",
        "Updated \\d+ minutes ago|Visitors: \\d+",
        "",
    ),
    text(
        "content_selector",
        "Watched area",
        "Compare only the text inside these elements: a tag, #id, .class or [attribute=value], several separated by commas, for example main or #content. Empty, or not found on a page: the whole page.",
        "main, #content",
        "",
    ),
    text(
        "exclude_selector",
        "Excluded areas",
        "Leave the text of these elements out, same syntax: a cookie banner, a \"latest posts\" box, an ad slot. For example .cookie-banner, #sidebar.",
        ".cookie-banner, #sidebar",
        "",
    ),
    boolean(
        "keep_page_chrome",
        "Compare menus, header and footer",
        "Off by default: the site's navigation, header, footer and sidebars are the same on every page, so a new menu entry would mark every page as changed. Their links are still followed.",
        false,
    ),
    boolean(
        "ignore_query",
        "Ignore query strings",
        "Site scope: addresses that differ only by their ?… part are one page. Tracking parameters (utm_…, fbclid, gclid…) are always left out.",
        false,
    ),
    text(
        "user_agent",
        "User agent",
        "Sent instead of DumbMonit's own, for a site that turns robots away. Empty: DumbMonit/<version>.",
        "Mozilla/5.0 (compatible; DumbMonit)",
        "",
    ),
    number(
        "check_interval_minutes",
        "Check every (minutes)",
        "How often the pages are read and compared, from 5 to 10080 (a week). An unreachable start page is retried at every polling interval.",
        "60",
        "60",
    ),
    insecure_tls(
        "For a site of your own behind a self-signed certificate. Never for someone else's site.",
    ),
    ALLOW_PRIVATE_TARGETS,
];

/// Options lues par `website/options.rs` (`Options::from_target`).
pub(super) const WEBSITE_OPTIONS: &[OptionView] = &[
    text(
        "expect_text",
        "Expected text",
        "A phrase the page must show, such as your company name or \"Add to cart\". Case and spacing do not matter. Empty: not checked.",
        "Add to cart",
        "",
    ),
    number(
        "min_text_chars",
        "Blank page below (characters)",
        "Visible text shorter than this counts as a blank page (20 by default, so short legitimate pages pass), from 0 to 100000. 0 turns the check off.",
        "20",
        "20",
    ),
    number(
        "max_redirects",
        "Max redirects",
        "Redirects followed at most, from 0 to 30. Going from http to https or from example.com to www.example.com is normal and never an error; a loop or more redirects than this is.",
        "10",
        "10",
    ),
    text(
        "category",
        "Category",
        "A label of your own (Shop, Blog, Client: Acme…), shown with the site and in its metrics. Set by the CSV import, which also guesses one (E-commerce, Blog, Web app, Showcase) when the page gives a clear sign. Empty: none.",
        "Shop",
        "",
    ),
    text(
        "user_agent",
        "User agent",
        "Sent instead of DumbMonit's own, for a site that serves robots differently. Empty: DumbMonit/<version>.",
        "Mozilla/5.0 (compatible; DumbMonit)",
        "",
    ),
    boolean(
        "ignore_query",
        "Ignore query strings",
        "Leave ?utm_source=… and other URL parameters out when comparing the final address and the canonical URL.",
        true,
    ),
    grouped(
        number(
            "change_check_minutes",
            "Check for changes every (minutes)",
            "How often the title, description, canonical URL, final address and robots.txt are compared with the reference, from 0 to 10080 (a week). Up/down, blank page and expected text are checked at every polling interval regardless. A value seen once is confirmed at the next poll. Empty: the server default, 360 (DUMBMONIT_WEBSITE_CHANGE_CHECK_MINUTES). 0: at every poll.",
            "360",
            "360",
        ),
        "Pacing",
    ),
    grouped(
        number(
            "confirm_checks",
            "Confirm a change after (checks)",
            "A different title, description, canonical URL or other watched value is only reported once it was read this many times in a row, from 1 to 10. Keeps A/B tests and flaky caches quiet. Also applies to the sharp drop of the page text.",
            "2",
            "2",
        ),
        "Change detection",
    ),
    grouped(
        number(
            "text_drop_percent",
            "Near-empty page: text drop (%)",
            "Reports a page whose visible text falls by more than this percentage below its usual size (a 200+ character reference is needed), from 0 to 99, confirmed like any change. Catches a page that is no longer blank but almost. 0 turns the check off.",
            "80",
            "80",
        ),
        "Change detection",
    ),
    grouped(
        boolean(
            "watch_final_url",
            "Watch the final address",
            "Report when, after its redirects, the site lands somewhere else (another host or path). http to https, www and trailing slashes never count.",
            true,
        ),
        "Watched metadata",
    ),
    grouped(
        boolean("watch_title", "Watch the title", "Report a changed or missing <title>.", true),
        "Watched metadata",
    ),
    grouped(
        boolean(
            "watch_description",
            "Watch the meta description",
            "Report a changed or missing meta description.",
            true,
        ),
        "Watched metadata",
    ),
    grouped(
        boolean(
            "watch_canonical",
            "Watch the canonical URL",
            "Report a changed or missing canonical link.",
            true,
        ),
        "Watched metadata",
    ),
    grouped(
        boolean(
            "watch_robots",
            "Watch the robots directives",
            "Report a changed robots meta tag or X-Robots-Tag header (noindex, nofollow…).",
            true,
        ),
        "Watched metadata",
    ),
    grouped(
        boolean(
            "watch_open_graph",
            "Watch Open Graph",
            "Report a changed og:title or og:description.",
            true,
        ),
        "Watched metadata",
    ),
    grouped(
        boolean("watch_lang", "Watch the page language", "Report a changed <html lang>.", true),
        "Watched metadata",
    ),
    grouped(
        boolean(
            "watch_h1",
            "Watch the h1 count",
            "Report a different number of <h1> headings.",
            true,
        ),
        "Watched metadata",
    ),
    grouped(
        boolean(
            "watch_hreflang",
            "Watch hreflang",
            "Report a changed set of hreflang language alternates.",
            true,
        ),
        "Watched metadata",
    ),
    grouped(
        boolean(
            "watch_google_verification",
            "Watch the Google verification tag",
            "Report a changed or missing google-site-verification meta tag (losing it can cut access to Search Console).",
            true,
        ),
        "Watched metadata",
    ),
    grouped(
        number(
            "host_spacing_seconds",
            "Wait before the next request to this domain (seconds)",
            "Minimum gap after this site's check before any other check of a site on the same domain (www.example.com and shop.example.com share one), from 0 to 3600. Keeps a firewall or CDN from mistaking a list of sites for an attack. Empty: the server default, 2 (DUMBMONIT_WEBSITE_HOST_SPACING_SECS).",
            "2",
            "2",
        ),
        "Pacing",
    ),
    ALLOW_PRIVATE_TARGETS,
];
