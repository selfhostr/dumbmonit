/**
 * Presentation helpers shared by the Proxmox VE panels.
 *
 * Proxmox prints binary units everywhere in its own interface; matching it
 * means a number read here and a number read there are the same number.
 */
/** The tones a Led accepts — a narrower set than Plate's. */
import { m } from '#lib/paraglide/messages.js';

type SignalTone = 'signal' | 'advisory' | 'warning' | 'info' | 'ghost';

export { formatBinaryBytes as formatBytes } from '#lib/format.js';

export function formatPercent(value: number | null | undefined): string {
	if (value === null || value === undefined || !Number.isFinite(value)) return '—';
	return `${value >= 10 ? Math.round(value) : Math.round(value * 10) / 10} %`;
}

export type Fill = 'signal' | 'advisory' | 'warning';

/** Meter tone by fill: past 90 % is a warning, past 75 % an advisory. */
export function fillTone(percent: number | null | undefined, warn = 90, advise = 75): Fill {
	if (percent === null || percent === undefined || !Number.isFinite(percent)) return 'signal';
	if (percent >= warn) return 'warning';
	if (percent >= advise) return 'advisory';
	return 'signal';
}

export const FILL: Record<Fill, string> = {
	signal: 'bg-signal',
	advisory: 'bg-advisory',
	warning: 'bg-warning'
};

/** Ceph health as a word and a tone: 0 OK, 1 WARN, 2 ERR, anything else unknown. */
export function cephHealth(level: number | null, status: string | null): { tone: SignalTone; word: string } {
	if (level === 0) return { tone: 'signal', word: status ?? 'HEALTH_OK' };
	if (level === 1) return { tone: 'advisory', word: status ?? 'HEALTH_WARN' };
	if (level === 2) return { tone: 'warning', word: status ?? 'HEALTH_ERR' };
	return { tone: 'ghost', word: status ?? m.devicesb_proxmox_format_unknown() };
}
