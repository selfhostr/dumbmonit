//! Types partagés entre le serveur, les collecteurs et l'agent.
//!
//! Ce crate ne dépend d'aucun runtime ni d'aucune base de données : il définit
//! uniquement le contrat de données. C'est ce qui garantit qu'un collecteur SNMP,
//! l'agent distant et le serveur parlent tous exactement le même langage.

mod collector;
mod command;
mod credential;
mod description;
mod discovery;
pub mod env;
mod eventlog;
mod logs;
mod push;
mod relay_offline;
pub mod remediation;
mod sample;
pub mod script;
mod selfupdate;
mod sentinel;
mod target;
mod tls;
mod version;

pub use collector::{Collector, ProbeError};
pub use command::{
    AgentCommand, CMD_AGENT_UNINSTALL, CMD_CONTAINER_RESTART, CMD_CONTAINER_UPDATE, CMD_PROBE,
    COMMAND_MAX_AGE_SECS, COMMAND_RESULT_MAX_BYTES, COMMAND_RESULT_TRUNCATED, COMMANDS_PATH,
    CommandReport, CommandStatus, NetconfigCapture, ProbeJob, ProbeOutcome, RELAY_PATH,
    RELAY_POLL_HOLD_SECS, RELAY_RESYNC_HEADER, truncate_result,
};
pub use credential::{
    Credential, CredentialInput, Resolved, SnmpV3Auth, SnmpV3AuthInput, SnmpV3AuthProtocol,
    SnmpV3Privacy, SnmpV3PrivacyInput, SnmpV3PrivacyProtocol,
};
pub use description::{
    CredentialDescription, CredentialFieldDescription, KindDescription, Measures,
    MetricDescription, OptionColumn, OptionDescription, OptionItem, PortDescription, Requirement,
    SetupDescription, StateMap, Text, ViewDescription,
};
pub use discovery::{
    AddressForm, CMD_DISCOVERY_SCAN, Confidence, DISCOVERY_REPORT_PATH, DiscoveredHost,
    DiscoveredService, DiscoveryReport, DiscoveryScanJob, Fingerprint, FingerprintProbe, HttpMatch,
    MAX_DISCOVERY_HOSTS, PrefillSpec, PrefillValue, ScanDepth, ScanPhase, ScanProgress, Transport,
    VersionRule,
};
pub use eventlog::{
    EVENT_RULE_MAX_PER_MINUTE, EventIdRange, EventLevel, EventLogDropped, EventLogEntry,
    EventLogReport, EventLogRule, MAX_EVENT_ID_RANGES, MAX_EVENT_LOG_ENTRIES,
    MAX_EVENT_MESSAGE_CHARS, format_event_ids, parse_event_ids, truncate_message,
};

pub use logs::{
    LOGS_PATH, LogBatch, LogLine, LogsSetting, MAX_LOG_LINES, MAX_LOG_MESSAGE, level_rank,
};
pub use push::{
    AGENT_KEY_HEADER, AGENT_SECRET_HEADER, AGENT_SECRET_PREFIX, AgentIdentity, BINDING_MISMATCH,
    DeclaredMonitor, ENROLMENT_PENDING, ENROLMENT_REJECTED, GatewayCheck, GatewayState,
    INGEST_PATH, InstalledPackage, MAX_BATCH_SAMPLES, MAX_DECLARED_MONITORS,
    MAX_GATEWAY_CHECK_ERROR, MAX_INVENTORY_PACKAGES, MAX_SERVER_LINK_TEXT, PUSH_PROTOCOL_VERSION,
    PushAck, PushBatch, RelaySetting, ServerLink, SoftwareInventory, TOKEN_PREFIX,
};
pub use relay_offline::{
    BacklogAck, BacklogResult, MAX_BACKLOG_RESULTS, PlannedProbe, RELAY_BACKLOG_PATH,
    RELAY_PLAN_PATH, RelayBacklog, RelayPlan,
};
pub use remediation::{CMD_SCRIPT_RUN, CMD_SERVICE_RESTART, DeclaredScript, RemediationInfo};
pub use sample::{MetricKind, Sample};
pub use script::{CheckMetric, SCRIPT_CHECK_KIND, ScriptCheck, ScriptResult};
pub use selfupdate::{
    AGENT_FILE_PREFIX, AgentUpdateOffer, SIGNATURE_SUFFIX, SelfUpdateAbility, SelfUpdateInfo,
    SelfUpdateOutcome, SelfUpdateReport, UPDATE_DOWNLOAD_PATH, UPDATE_TRIAL_SECS, agent_file_name,
};
pub use sentinel::{
    SENTINEL_MAX_INTERVAL_SECS, SENTINEL_MAX_THRESHOLD, SENTINEL_MAX_URL_LEN, SENTINEL_MAX_URLS,
    SENTINEL_MIN_INTERVAL_SECS, SentinelChannel, SentinelSetting, https_authority, normalize_pin,
};
pub use target::{
    EXPORTED_LABEL_PREFIX, MAX_TAG_LABEL_LEN, TAG_LABEL_PREFIX, Target, TargetId, apply_identity,
    looks_secret,
};
pub use tls::{
    AGENT_ALPN, AGENT_TLS_HEADER, AGENT_TLS_SERVER_NAME, AGENT_WS_CHUNK, AGENT_WS_MAX_MESSAGE,
    AGENT_WS_PATH, AGENT_WS_PROTOCOL, CERT_REFUSED, CLIENT_CERT_DAYS, DEFAULT_AGENT_TLS_PORT,
    DEFAULT_RELAY_GATEWAY_PORT, MTLS_REQUIRED, RELAY_MUX_PROTOCOL, RELAY_TUNNEL_PATH,
    RELAY_TUNNEL_PROTOCOL, RENEW_BEFORE_DAYS, TLS_ENROL_PATH, TLS_RENEW_PATH, TlsCertificate,
    TlsEnrolRequest, TlsOffer, TlsRenewRequest, agent_channel_url, is_pin, normalise_pin,
};
pub use version::{Version, is_older};
