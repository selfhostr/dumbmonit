// --- Redfish -------------------------------------------------------------------
// Mirrors `crates/server/src/api/redfish.rs`: a server's hardware as its
// management controller reports it, rebuilt from the last `dumbmonit_redfish_*`
// series. Health values are 0 OK, 1 Warning, 2 Critical; `null` is unknown.

export interface RedfishService {
	vendor: string | null;
	product: string | null;
	redfish_version: string | null;
}

export interface RedfishSystem {
	id: string;
	/** The system's own health. */
	health: number | null;
	/** Everything it contains, rolled up. */
	health_rollup: number | null;
	power_on: boolean | null;
	/** The controller's summary for every DIMM. */
	memory_health: number | null;
	/** The controller's summary for every CPU. */
	processor_health: number | null;
	memory_total_bytes: number | null;
}

export interface RedfishChassis {
	id: string;
	health: number | null;
	health_rollup: number | null;
	power_consumed_watts: number | null;
}

/** A reading against the thresholds the controller declares for that sensor. */
export type RedfishLimit = 'within' | 'caution' | 'critical';

export interface RedfishTemperature {
	chassis: string;
	sensor: string;
	celsius: number | null;
	upper_caution_celsius: number | null;
	upper_critical_celsius: number | null;
	health: number | null;
	/** `null` when the sensor declares no threshold, or has no reading. */
	limit: RedfishLimit | null;
}

export interface RedfishFan {
	chassis: string;
	fan: string;
	rpm: number | null;
	/** When the controller reports a percentage instead of RPM. */
	percent: number | null;
	lower_critical_rpm: number | null;
	lower_critical_percent: number | null;
	health: number | null;
	/** `null` when the fan declares no lower threshold. */
	limit: RedfishLimit | null;
}

export interface RedfishRedundancy {
	chassis: string;
	group: string;
	health: number | null;
}

export interface RedfishPsu {
	chassis: string;
	psu: string;
	health: number | null;
	output_watts: number | null;
	capacity_watts: number | null;
}

export interface RedfishVoltage {
	chassis: string;
	sensor: string;
	volts: number | null;
	health: number | null;
}

export interface RedfishStorage {
	system: string;
	storage: string;
	health: number | null;
}

export interface RedfishDrive {
	system: string;
	drive: string;
	/** `HDD`, `SSD`… as the controller declares it; empty when it says nothing. */
	media: string;
	health: number | null;
	failure_predicted: boolean | null;
	life_left_percent: number | null;
	capacity_bytes: number | null;
}

export interface RedfishManager {
	id: string;
	health: number | null;
	firmware: string | null;
	model: string | null;
}

export interface RedfishLog {
	/** The system or controller that keeps this log. */
	owner: string;
	log: string;
	entries: number | null;
	critical: number | null;
	warning: number | null;
}

export interface RedfishOverview {
	service: RedfishService;
	systems: RedfishSystem[];
	chassis: RedfishChassis[];
	temperatures: RedfishTemperature[];
	fans: RedfishFan[];
	fan_redundancy: RedfishRedundancy[];
	power_supplies: RedfishPsu[];
	power_redundancy: RedfishRedundancy[];
	voltages: RedfishVoltage[];
	storage: RedfishStorage[];
	drives: RedfishDrive[];
	managers: RedfishManager[];
	logs: RedfishLog[];
	/** Resources the last read could not get. */
	scrape_errors: number | null;
	/** Unix seconds of the most recent sample; `null` before the first probe. */
	sampled_at: number | null;
}

// --- Log and metrics servers ---------------------------------------------------
// Mirrors `crates/server/src/api/observability.rs`: VictoriaMetrics,
// VictoriaLogs, Loki and Graylog, rebuilt from their last `dumbmonit_<kind>_*`
// series. The server writes the verdicts; the panel only shows them.

export type ObservabilityState = 'ok' | 'advisory' | 'warning';

export type ObservabilityUnit = 'bytes' | 'bytes_per_second' | 'per_second' | 'percent' | 'count' | 'seconds';

export interface ObservabilityCheck {
	label: string;
	state: ObservabilityState;
	detail: string;
}

export interface ObservabilityFigure {
	label: string;
	value: number | null;
	unit: ObservabilityUnit;
}

export interface ObservabilityRow {
	label: string;
	value: number | null;
	unit: ObservabilityUnit;
	/** `null` for a row without a verdict. */
	state: ObservabilityState | null;
}

export interface ObservabilityBreakdown {
	title: string;
	/** One sentence under the title; empty when there is none. */
	note: string;
	rows: ObservabilityRow[];
}

export interface ObservabilityOverview {
	kind: string;
	version: string | null;
	/** Unix seconds of the most recent sample; `null` before the first probe. */
	sampled_at: number | null;
	/** The worst of the checks. */
	state: ObservabilityState;
	checks: ObservabilityCheck[];
	figures: ObservabilityFigure[];
	breakdowns: ObservabilityBreakdown[];
}

// --- Integration packs (crates/server/src/api/packs.rs) ------------------------

/** What an install changed (`packs::Saved`). */
export type PackSaved = 'created' | 'updated' | 'unchanged';

/** An installed integration pack (`PackView`). */
export interface PackView {
	id: string;
	version: string;
	label: string;
	summary: string;
	/** Device type it adds (`pack.<id>`); `null` for a pack that only brings SNMP profiles. */
	kind: string | null;
	enabled: boolean;
	/** Server timestamp, UTC without suffix. */
	installed_at: string;
	sha256: string;
	/** Full names of the metrics it produces. */
	metrics: string[];
	/** `uid` of the alert rules it ships. */
	rules: string[];
	snmp_profiles: string[];
	/** Devices that use this type. */
	targets: number;
	warnings: string[];
	/** Set when the stored YAML no longer passes validation: the pack is then inactive. */
	error: string | null;
}

/** Answer to `POST /api/packs` (`InstallReport`). */
export interface PackInstallReport {
	outcome: PackSaved;
	pack: PackView;
	/** Rules added; a rule already present is never rewritten. */
	rules_added: number;
	/** The pack brings SNMP profiles: they are read at start-up only. */
	restart_required: boolean;
}

// --- Template import (crates/server/src/api/template_import.rs) ---------------

/** Format recognised in an imported file (`SourceFormat`). */
export type TemplateFormat =
	| 'zabbix_yaml'
	| 'zabbix_xml'
	| 'snmp_exporter_generator'
	| 'snmp_exporter_snmp';

/** Body of `POST /api/import/templates/preview` and `/apply`. */
export interface TemplateImportRequest {
	/** The file's text, at most 2 MiB. */
	content: string;
	filename?: string;
	/** Pack id; derived from the template name when omitted. */
	id?: string;
	label?: string;
	/** sysObjectID prefixes the SNMP profiles apply to. */
	sysobjectid?: string[];
}

/** One element of the imported file and what became of it (`ReportEntry`). */
export interface TemplateImportEntry {
	element: string;
	name: string;
	outcome: 'converted' | 'partial' | 'ignored';
	/** English text from the server. */
	reason: string;
}

export interface TemplateImportSummary {
	snmp_profiles: string[];
	snmp_metrics: number;
	http_sources: number;
	http_metrics: number;
	/** Rule uids. */
	rules: string[];
	options: number;
	converted: number;
	partial: number;
	ignored: number;
}

/** Answer to the preview (`Preview`): nothing is stored yet. */
export interface TemplateImportPreview {
	format: TemplateFormat;
	pack_id: string;
	label: string;
	/** The pack that applying installs. */
	yaml: string;
	summary: TemplateImportSummary;
	report: TemplateImportEntry[];
	/** Why the pack cannot be installed; empty when it can. */
	errors: string[];
	installable: boolean;
	/** A pack with this id is installed: applying updates it. */
	exists: boolean;
}

/** Answer to the apply (`Applied`). */
export interface TemplateImportApplied {
	pack_id: string;
	kind: string | null;
	snmp_profiles: string[];
	rules: string[];
	install: PackInstallReport;
}
