// --- First-run guide --------------------------------------------------------

/**
 * Where the instance stands on the three first-run steps, as the server sees
 * it. `complete` is latched — set when the three steps are settled, or on the
 * very first read of an instance that already had a device and a channel — and
 * never goes back to false, so the guide never returns.
 */
export interface OnboardingState {
	/** The guide was skipped, for every browser and every user. */
	skipped: boolean;
	complete: boolean;
	/** UTC without suffix; `null` until the guide completes. */
	completed_at: string | null;
	/** Ids of the steps dismissed one by one: `device`, `channel`, `test`. */
	dismissed: OnboardingStep[];
	has_target: boolean;
	has_channel: boolean;
	/** A message really left on some channel — a test counts. */
	notification_confirmed: boolean;
	/** When the instance first passed each step. Older servers omit it. */
	first_run?: FirstRunTimings;
}

/**
 * First-run stopwatch, kept on the instance only. UTC without suffix (read with
 * `parseServerDate`), `null` until the step happens. Mirrors `first_run::Timings`.
 */
export interface FirstRunTimings {
	first_boot_at: string | null;
	setup_at: string | null;
	first_target_at: string | null;
	first_channel_at: string | null;
	first_notification_at: string | null;
	first_alert_at: string | null;
}

export type OnboardingStep = 'device' | 'channel' | 'test';

export interface OnboardingPatch {
	skipped?: boolean;
	dismissed?: OnboardingStep[];
}

// --- Configuration files (GitOps) ---------------------------------------------

/** One line of a configuration plan. Mirrors `Change` in `crates/server/src/gitops/plan.rs`. */
export interface ConfigChange {
	section: 'targets' | 'channels' | 'rules' | 'rule_overrides' | 'silences' | 'status_pages';
	key: string;
	action: 'create' | 'update' | 'delete';
	/** Names of the fields that differ, never their values. */
	fields: string[];
}

/** Response of `POST /api/config/plan` and `/api/config/apply`. */
export interface ConfigPlan {
	changes: ConfigChange[];
	create: number;
	update: number;
	delete: number;
	unchanged: number;
	warnings: string[];
	applied: boolean;
	/** Objects whose secret references could not be resolved (plan only). */
	unresolved: string[];
}

/** Response of `GET /api/config/export?format=json`. */
export interface ConfigExport {
	version: number;
	yaml: string;
	config: Record<string, unknown>;
	/** Environment variables to define before applying the file. */
	variables: string[];
}

/** Response of `GET /api/config/status`. */
export interface ConfigStatus {
	schema_version: number;
	watch: {
		directory: string | null;
		files: string[];
		last_applied_at: string | null;
		last_error: string | null;
		/** `[created, updated, deleted]` of the last application. */
		last_changes: [number, number, number] | null;
	};
	managed: Record<string, number>;
	/** Keys of the file-managed objects by kind: rule (uid), channel (name), silence, status_page (slug), override. */
	keys: Record<string, string[]>;
}

// --- Instance settings and setup wizard -------------------------------------

/** `GET /api/instance`: public, read before sign-in to know whether to offer the wizard. */
export interface PublicInstance {
	setup_completed: boolean;
}

/** `GET /api/instance/settings`. Mirrors `InstanceSettings` in `crates/server/src/instance.rs`. */
export interface InstanceSettings {
	setup_completed: boolean;
	/** UTC without suffix; `null` until the wizard is finished. */
	setup_completed_at?: string | null;
	/** Empty shows "DumbMonit". At most 80 characters. */
	name: string;
	/** Default interface language (a Paraglide locale); empty follows the browser. */
	language: string;
	/** IANA time zone; empty means UTC. */
	timezone: string;
	/** On-call schedules are turned on. Off by default; schedules are kept while off. */
	oncall_enabled?: boolean;
	/** Accent colour of the interface as `#rrggbb`; empty or missing is the default look. */
	accent?: string;
}

/** `PUT /api/instance/settings`: an omitted field keeps its value. */
export interface InstancePatch {
	setup_completed?: boolean;
	name?: string;
	language?: string;
	timezone?: string;
	oncall_enabled?: boolean;
	/** `#rrggbb`, or empty for the default look. */
	accent?: string;
}

// --- Backup and restore -----------------------------------------------------

/** One section of the backup bundle. Mirrors `Section` in `crates/server/src/api/backup.rs`. */
export interface BackupSection {
	section: string;
	description: string;
	count: number;
}

/** A backup file sitting in the local backup directory. */
export interface BackupFile {
	name: string;
	bytes: number;
	/** UTC without suffix, read from the file name. */
	at: string;
	/** The matching `secret.key` copy is next to it. */
	with_secret: boolean;
}

/** What one scheduled run did. `null` fields mean it never ran. */
export interface BackupRun {
	at: string;
	ok: boolean;
	file: string;
	bytes: number;
	duration_ms: number;
	with_secret: boolean;
	error: string | null;
}

/** State of the scheduled local backups. Mirrors `local::Status`. */
export interface BackupSchedule {
	enabled: boolean;
	directory: string;
	interval_hours: number;
	keep: number;
	/** False when the instance secret comes from `DUMBMONIT_SECRET`: nothing to copy. */
	includes_secret_key: boolean;
	last_run: BackupRun | null;
	files: BackupFile[];
	total_bytes: number;
	directory_error: string | null;
	/** Settings a `DUMBMONIT_BACKUP_*` variable fixes: the interface cannot change them. */
	env_managed: { enabled: boolean; interval_hours: boolean; keep: boolean };
}

/** Response of `GET /api/backup`. */
export interface BackupStatus {
	bundle_version: number;
	min_passphrase_length: number;
	contents: BackupSection[];
	/** `file` when the secret lives in `secret.key`, `environment` otherwise. */
	secret_source: 'file' | 'environment';
	secret_path: string;
	schedule: BackupSchedule;
}

/** The cleartext header of a bundle: readable without the passphrase. */
export interface BackupEnvelope {
	format: string;
	version: number;
	created_at: string;
	source_version: string;
	summary: Record<string, number>;
	cipher: string;
	payload: string;
	[key: string]: unknown;
}

/** What one section of a restore produced. Mirrors `SectionReport`. */
export interface RestoreSection {
	section: string;
	created: number;
	updated: number;
	skipped: number;
	notes: string[];
}

/** Response of `POST /api/backup/restore`. Mirrors `RestoreReport`. */
export interface RestoreReport {
	/** False for a dry run: nothing was written. */
	applied: boolean;
	created_at: string;
	source_version: string;
	sections: RestoreSection[];
	created: number;
	updated: number;
	skipped: number;
	warnings: string[];
}

/** The optional modules. Mirrors `Features` in `crates/server/src/features.rs`. */
export interface Features {
	/** Wall mode (`/wall` and wall-display accounts). */
	wall: boolean;
	/** Scheduled reports and SLA reports. */
	sla_reports: boolean;
	/** Status pages and their incidents, public pages included. */
	status_pages: boolean;
	/** Service level objectives. */
	slo: boolean;
	/** World map of attacks in the Security area. */
	threat_map: boolean;
}

/** `PUT /api/settings/features`: an omitted field keeps its value. */
export type FeaturesPatch = Partial<Features>;
