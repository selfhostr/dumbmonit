/**
 * "Your first alert": the pure part of the wizard's first step, tested without
 * a browser (`tests/first-alert.test.mjs`).
 *
 * One field says what to watch; one click says where the alert goes; one
 * button proves the chain. Everything else waits for later screens.
 */
import type { FirstRunTimings, TargetPayload } from '#lib/api/types.js';

/** Host of an address, without scheme, port, path or brackets (as `add-by-site.ts`). */
function hostOf(address: string): string {
	const bare = address.trim().replace(/^[a-z][a-z0-9+.-]*:\/\//i, '');
	const host = bare.split(/[/?#]/)[0] ?? '';
	if (host.startsWith('[')) return host.slice(1, host.indexOf(']')).toLowerCase();
	return host.replace(/:\d+$/, '').toLowerCase();
}

/**
 * Channel kinds offered in one click, simplest first: ntfy needs nothing but a
 * topic (filled in for you), Discord and Telegram a link or a token, email an
 * SMTP server, webhook an URL. The others stay one click away ("Other").
 * Only the kinds the server actually lists are shown.
 */
export const QUICK_CHANNEL_KINDS = ['ntfy', 'discord', 'telegram', 'smtp', 'webhook'] as const;

/** The quick kinds the server catalogue has, in the order above. */
export function quickKinds<K extends { kind: string }>(catalogue: K[]): K[] {
	return QUICK_CHANNEL_KINDS.map((kind) => catalogue.find((k) => k.kind === kind)).filter(
		(k): k is K => k !== undefined
	);
}

/**
 * What the one field of "What should we watch?" becomes.
 *
 * - An URL (`http://`, `https://`) is a website check (`http`).
 * - A host name or an IP, bare, is a ping (`ping`).
 * - `host:port` is a TCP port check (`tcp`), when the server has it.
 *
 * `null` when the text is empty, has spaces, or the matching kind is missing:
 * the full form then takes over.
 */
export function guessMonitor(input: string, kinds: Iterable<string>): TargetPayload | null {
	const text = input.trim();
	if (!text || /\s/.test(text)) return null;
	const available = new Set(kinds);

	if (/^https?:\/\//i.test(text)) {
		const host = hostOf(text);
		if (!host || !available.has('http')) return null;
		return { name: host, address: text, kind: 'http' };
	}
	if (/^[a-z][a-z0-9+.-]*:\/\//i.test(text)) return null;

	// `nas.lan:8080`, `10.0.0.5:22`, `[fd00::5]:443`: a port is named.
	const port = /^(\[[0-9a-f:.]+\]|[^:/]+):(\d{1,5})$/i.exec(text);
	if (port) {
		const number = Number(port[2]);
		if (number < 1 || number > 65535 || !available.has('tcp')) return null;
		return { name: hostOf(text), address: text, kind: 'tcp' };
	}

	// A host name or an IP: letters, digits, dots, dashes, or an IPv6 address.
	if (!/^[a-z0-9.-]+$/i.test(text) && !/^[0-9a-f:]+$/i.test(text)) return null;
	if (!available.has('ping')) return null;
	return { name: text.toLowerCase(), address: text, kind: 'ping' };
}

/** A long random ntfy topic: on ntfy.sh, whoever knows the name reads the topic. */
export function ntfyTopic(random: (bytes: Uint8Array) => Uint8Array = (b) => crypto.getRandomValues(b)): string {
	// 32 symbols: the low 5 bits of each byte pick one, with no bias (no modulo).
	const alphabet = 'abcdefghijkmnpqrstuvwxyz23456789';
	const bytes = random(new Uint8Array(14));
	let topic = '';
	for (const byte of bytes) topic += alphabet[byte & 0x1f];
	return `dumbmonit-${topic}`;
}

/** Page to open on the phone to subscribe (the ntfy app opens the same link). */
export function ntfySubscribeUrl(server: unknown, topic: unknown): string | null {
	if (typeof topic !== 'string' || !topic.trim()) return null;
	const base = typeof server === 'string' && /^https?:\/\//i.test(server.trim()) ? server.trim() : 'https://ntfy.sh';
	return `${base.replace(/\/+$/, '')}/${encodeURIComponent(topic.trim())}`;
}

/**
 * Upper bound, in minutes, between a monitor going down and the message, with
 * the shipped rules and the default 60 s interval:
 *
 * - a website, ping or port check: the next probe fails (up to 60 s), the
 *   next evaluation sees it (up to 30 s), `service_down` waits two minutes of
 *   failures, the batch window 30 s: 2.5 to 4 minutes;
 * - any other monitor: no measurement for three minutes (the last one came up
 *   to 60 s before the failure), one more cycle to confirm, the batch window:
 *   3 to 4.5 minutes.
 */
export function expectedAlertMinutes(kind: string): number {
	return kind === 'http' || kind === 'ping' || kind === 'tcp' ? 4 : 5;
}

/** One line of the first-run stopwatch in Settings → About. */
export interface FirstRunStep {
	key: keyof FirstRunTimings;
	at: Date;
	/** Seconds since the start of the measure (first boot, or the account when it is missing). */
	elapsed: number;
}

const ORDER: (keyof FirstRunTimings)[] = [
	'first_boot_at',
	'setup_at',
	'first_target_at',
	'first_channel_at',
	'first_notification_at',
	'first_alert_at'
];

/** The steps passed, in their order, each with its delay since the start. */
export function firstRunSteps(timings: FirstRunTimings | null | undefined, parse: (value: string) => Date | null): FirstRunStep[] {
	if (!timings) return [];
	const steps: { key: keyof FirstRunTimings; at: Date }[] = [];
	for (const key of ORDER) {
		const raw = timings[key];
		const at = raw ? parse(raw) : null;
		if (at) steps.push({ key, at });
	}
	if (steps.length === 0) return [];
	const start = steps[0].at.getTime();
	return steps.map((step) => ({ ...step, elapsed: Math.max(0, Math.round((step.at.getTime() - start) / 1000)) }));
}
