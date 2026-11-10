/**
 * Tone and state of a day in the public history bar, from its uptime. The
 * tooltip over a segment names the state in words, so colour is never alone.
 */
export type DayTone = 'signal' | 'advisory' | 'warning' | 'ghost';

/** At or above: a good day. */
export const DAY_GOOD = 99.5;
/** At or above (and under `DAY_GOOD`): a day with some downtime; below: a poor one. */
export const DAY_POOR = 95;

export function dayTone(uptime: number | null): DayTone {
	if (uptime === null) return 'ghost';
	if (uptime >= DAY_GOOD) return 'signal';
	if (uptime >= DAY_POOR) return 'advisory';
	return 'warning';
}


/** Under this a day is a major outage; between it and `DAY_POOR`, a partial one. */
export const DAY_MAJOR = 50;

/** State of a day, in the words of the tooltip over a history segment. */
export type DayState = 'operational' | 'degraded' | 'partial' | 'major' | 'nodata';

export function dayState(uptime: number | null): DayState {
	if (uptime === null) return 'nodata';
	if (uptime >= DAY_GOOD) return 'operational';
	if (uptime >= DAY_POOR) return 'degraded';
	if (uptime >= DAY_MAJOR) return 'partial';
	return 'major';
}
