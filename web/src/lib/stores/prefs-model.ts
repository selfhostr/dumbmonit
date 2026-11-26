/**
 * Display preferences kept per browser (Settings › Appearance › Display).
 * Pure (no DOM, no framework), so `node --test` loads it directly.
 */

export type TimeFormat = 'auto' | '12' | '24';
export type WeekStart = 'auto' | 'monday' | 'sunday';
export type Density = 'comfortable' | 'compact';
export type MotionPref = 'auto' | 'reduce';

export interface Prefs {
	timeFormat: TimeFormat;
	weekStart: WeekStart;
	density: Density;
	motion: MotionPref;
	/** The GitHub and documentation buttons at the foot of the sidebar. */
	showGithub: boolean;
	showDocs: boolean;
}

export const PREFS_KEY = 'dumbmonit-display-prefs';

export const DEFAULT_PREFS: Prefs = {
	timeFormat: 'auto',
	weekStart: 'auto',
	density: 'comfortable',
	motion: 'auto',
	showGithub: true,
	showDocs: true
};

/** What was stored, field by field; anything unknown falls back to the default. */
export function parsePrefs(raw: string | null | undefined): Prefs {
	const out: Prefs = { ...DEFAULT_PREFS };
	if (!raw) return out;
	try {
		const data = JSON.parse(raw) as Record<string, unknown> | null;
		if (!data || typeof data !== 'object') return out;
		if (data.timeFormat === '12' || data.timeFormat === '24') out.timeFormat = data.timeFormat;
		if (data.weekStart === 'monday' || data.weekStart === 'sunday') out.weekStart = data.weekStart;
		if (data.density === 'compact') out.density = 'compact';
		if (data.motion === 'reduce') out.motion = 'reduce';
		if (data.showGithub === false) out.showGithub = false;
		if (data.showDocs === false) out.showDocs = false;
	} catch {
		// Malformed: defaults.
	}
	return out;
}

/** `Intl` `hour12` option for the time format, `undefined` to follow the locale. */
export function hour12Option(format: TimeFormat): boolean | undefined {
	return format === 'auto' ? undefined : format === '12';
}

/** First day of the week (0 Sunday, 1 Monday), `null` to follow the locale. */
export function firstDayOverride(start: WeekStart): number | null {
	return start === 'monday' ? 1 : start === 'sunday' ? 0 : null;
}
