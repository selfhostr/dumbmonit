/**
 * Sub-monitors folded inside their parent in the monitors table.
 *
 * The relation is `Target.parent_id`: a service an agent discovers (Docker,
 * ZFS, Plakar… `agent_service`), a monitor declared by an agent's Docker
 * labels, or a parent chosen by hand. `buildRack` already lists the children
 * right under their parent, one depth deeper; this module only decides which
 * of those rows are on screen. A parent shows a disclosure with its count of
 * direct sub-monitors; its children appear when it is open.
 *
 * Most monitors have no `parent_id` though: a PostgreSQL, a Redis or a web
 * check aimed at a server already in the list. `inferHosts` puts them inside
 * that server for display only (nothing is saved, alerts are not suppressed
 * by it): same host name or IP address as exactly one machine monitor.
 *
 * While the list is filtered, a matching sub-monitor brings its parents along
 * (`withAncestors`, flagged `context`) so it is still shown where it lives.
 *
 * Which parents are open is a per-viewer convenience kept in `localStorage`
 * (every access guarded: private windows and blocked storage throw).
 */
import type { Target, TargetId } from '#lib/api/index.js';
// Explicit extension: this module is also loaded straight by Node (`npm test`).
import { needsAttention, type RackRow } from './rack.ts';

/**
 * How surely a kind stands for the machine itself rather than a service on
 * it: an agent first, then a hypervisor, a NAS, a firewall or a Windows
 * host, then a bare SNMP device, a ping last. A monitor that already has
 * sub-monitors of its own counts as a machine too, at the first rank.
 */
const HOST_RANK: Record<string, number> = {
	agent: 0,
	winrm: 1,
	proxmox: 1,
	xcpng: 1,
	nutanix: 1,
	synology: 1,
	truenas: 1,
	unraid: 1,
	qnap: 1,
	pbs: 1,
	pmg: 1,
	opnsense: 1,
	pfsense: 1,
	fortigate: 1,
	sophos: 1,
	'pack.paloalto': 1,
	'pack.sonicwall': 1,
	mikrotik: 1,
	snmp: 2,
	ping: 3
};

/**
 * Kinds whose address names no machine on the network: an agent's is its
 * identity (its host name is its name), a service's is `machine#module`, the
 * others are accounts, domains or heartbeats.
 */
const NOT_A_HOST = new Set([
	'agent',
	'agent_service',
	'push',
	'dummy',
	'otlp',
	'domain',
	'provider',
	'ip_blacklist',
	'mail_blacklist',
	'aws',
	'gcp',
	'azure',
	'm365',
	'entra',
	'cloudflare',
	'hetzner',
	'ovh',
	'infomaniak',
	'tailscale',
	'arubacentral'
]);

const IPV4 = /^\d{1,3}(?:\.\d{1,3}){3}$/;

/**
 * The host an address or a URL points at, in lower case, without scheme,
 * credentials, port or path: `postgres://u@db.lan:5432/app` → `db.lan`. `null`
 * for anything that is not a host name or an IP address, and for the loopback
 * (`localhost` is every machine).
 */
export function hostOf(text: string): string | null {
	let s = text.trim().toLowerCase();
	const scheme = s.indexOf('://');
	if (scheme >= 0) s = s.slice(scheme + 3);
	s = s.split(/[/?#]/)[0];
	const at = s.lastIndexOf('@');
	if (at >= 0) s = s.slice(at + 1);
	let host: string;
	if (s.startsWith('[')) host = s.slice(1, s.indexOf(']') > 0 ? s.indexOf(']') : undefined);
	else if (s.split(':').length === 2) host = s.split(':')[0];
	else host = s;
	host = host.replace(/\.$/, '');
	if (host === '') return null;
	if (host.includes(':')) {
		// IPv6.
		if (!/^[0-9a-f:.]+$/.test(host) || host === '::1' || host === '::') return null;
		return host;
	}
	if (IPV4.test(host)) return host.startsWith('127.') || host === '0.0.0.0' ? null : host;
	if (!/^[a-z0-9_-]+(?:\.[a-z0-9_-]+)*$/.test(host)) return null;
	if (host === 'localhost' || host.endsWith('.localhost')) return null;
	return host;
}

const isIp = (host: string) => host.includes(':') || IPV4.test(host);

/**
 * Display-only parents: every monitor without a `parent_id` whose host (from
 * its address) is the host of exactly one machine monitor of the same folder
 * goes inside it. The machine's host is its address, or for an agent its name
 * (the host name it reported). `db.lan` and a machine named `db` are the same
 * host; two different domains are not. Among several machines at one host the
 * surest kind wins (`HOST_RANK`): a ping beside a Proxmox VE goes inside it.
 * A tie is no parent at all, rather than a wrong one.
 *
 * Returns the targets with the inferred `parent_id` filled in (the others
 * untouched, same objects) and the ids that got one. A declared parent always
 * stays; a monitor never lands under itself nor under one of its own
 * sub-monitors.
 */
export function inferHosts(targets: Target[]): { targets: Target[]; inferred: Set<TargetId> } {
	const byId = new Map(targets.map((t) => [t.id, t]));
	const parentOf = new Map<TargetId, TargetId | null>(
		targets.map((t) => [t.id, t.parent_id !== null && byId.has(t.parent_id) ? t.parent_id : null])
	);
	const hasChildren = new Set<TargetId>();
	for (const parent of parentOf.values()) if (parent !== null) hasChildren.add(parent);
	const folderOf = (t: Target): string => {
		let current = t;
		const seen = new Set<TargetId>([t.id]);
		for (let up = parentOf.get(current.id); up != null && !seen.has(up); up = parentOf.get(current.id)) {
			seen.add(up);
			current = byId.get(up)!;
		}
		return current.group_name || '';
	};
	const rankOf = (t: Target): number =>
		hasChildren.has(t.id) ? 0 : (HOST_RANK[t.kind] ?? Infinity);
	const hostOfTarget = (t: Target): string | null =>
		t.kind === 'agent' ? hostOf(t.name) : NOT_A_HOST.has(t.kind) ? null : hostOf(t.address);

	// Machines by their host: exactly, and by the first label of a name.
	const exact = new Map<string, Target[]>();
	const bare = new Map<string, Target[]>();
	const dotted = new Map<string, Target[]>();
	const push = (map: Map<string, Target[]>, key: string, t: Target) => {
		const list = map.get(key);
		if (list) list.push(t);
		else map.set(key, [t]);
	};
	for (const t of targets) {
		if (rankOf(t) === Infinity) continue;
		const host = hostOfTarget(t);
		if (host === null) continue;
		push(exact, host, t);
		if (isIp(host)) continue;
		const dot = host.indexOf('.');
		if (dot < 0) push(bare, host, t);
		else push(dotted, host.slice(0, dot), t);
	}

	const out = new Map<TargetId, Target>();
	const inferred = new Set<TargetId>();
	/** True when `id` sits under `ancestor`, declared or inferred so far. */
	const isUnder = (id: TargetId, ancestor: TargetId): boolean => {
		const seen = new Set<TargetId>();
		for (let up: TargetId | null | undefined = id; up != null && !seen.has(up); up = parentOf.get(up)) {
			if (up === ancestor) return true;
			seen.add(up);
		}
		return false;
	};
	for (const t of [...targets].sort((a, b) => a.id - b.id)) {
		if (parentOf.get(t.id) !== null) continue;
		const host = hostOfTarget(t);
		if (host === null) continue;
		const found = new Set(exact.get(host) ?? []);
		if (!isIp(host)) {
			// `db.lan` is the machine named `db`, and the other way round.
			const dot = host.indexOf('.');
			for (const c of (dot < 0 ? dotted.get(host) : bare.get(host.slice(0, dot))) ?? []) found.add(c);
		}
		found.delete(t);
		const own = rankOf(t);
		const folder = t.group_name || '';
		const fits = [...found].filter((c) => rankOf(c) < own && folderOf(c) === folder);
		if (fits.length === 0) continue;
		const best = Math.min(...fits.map(rankOf));
		const top = fits.filter((c) => rankOf(c) === best);
		if (top.length !== 1) continue;
		const parent = top[0];
		if (isUnder(parent.id, t.id)) continue;
		parentOf.set(t.id, parent.id);
		inferred.add(t.id);
		out.set(t.id, { ...t, parent_id: parent.id });
	}
	return {
		targets: inferred.size === 0 ? targets : targets.map((t) => out.get(t.id) ?? t),
		inferred
	};
}

/**
 * `ids` plus every ancestor of theirs, as long as the ancestor is `eligible`
 * (the same section of the list: a disabled parent is not pulled into the
 * active monitors). Stops at the first ineligible ancestor.
 */
export function withAncestors(
	targets: Target[],
	ids: ReadonlySet<TargetId>,
	eligible: ReadonlySet<TargetId>
): Set<TargetId> {
	const byId = new Map(targets.map((t) => [t.id, t]));
	const out = new Set(ids);
	for (const id of ids) {
		const seen = new Set<TargetId>([id]);
		let parent = byId.get(id)?.parent_id ?? null;
		while (parent !== null && byId.has(parent) && !seen.has(parent) && eligible.has(parent)) {
			seen.add(parent);
			out.add(parent);
			parent = byId.get(parent)?.parent_id ?? null;
		}
	}
	return out;
}

/** Flags the rows that are not among `matching`: parents brought along by `withAncestors`. */
export function markContext(rows: RackRow[], matching: ReadonlySet<TargetId>): RackRow[] {
	return rows.map((row) => (matching.has(row.target.id) ? row : { ...row, context: true }));
}

/**
 * A sorted flat list put back into its tree: every row keeps its place among
 * its siblings, children go right under their parent, one depth deeper. A
 * row whose parent is not listed stays at the top.
 */
export function nestSorted(rows: RackRow[]): RackRow[] {
	const listed = new Set(rows.map((r) => r.target.id));
	const under = new Map<TargetId | null, RackRow[]>();
	for (const row of rows) {
		const parent = row.target.parent_id ?? null;
		const key = parent !== null && listed.has(parent) ? parent : null;
		const list = under.get(key);
		if (list) list.push(row);
		else under.set(key, [row]);
	}
	const out: RackRow[] = [];
	const seen = new Set<TargetId>();
	const walk = (parent: TargetId | null, depth: number) => {
		for (const row of under.get(parent) ?? []) {
			if (seen.has(row.target.id)) continue;
			seen.add(row.target.id);
			out.push({ ...row, depth });
			walk(row.target.id, depth + 1);
		}
	};
	walk(null, 0);
	// A cycle the server would not normally allow: listed flat rather than lost.
	for (const row of rows) {
		if (seen.has(row.target.id)) continue;
		seen.add(row.target.id);
		out.push({ ...row, depth: 0 });
		walk(row.target.id, 1);
	}
	return out;
}

/**
 * The rows on screen: `rows` in tree order (children right after their
 * parent, deeper), each with its count of direct sub-monitors and of those
 * needing attention below it; the rows under a closed parent are left out.
 * `isOpen` decides for every row that has sub-monitors.
 */
export function foldRack(
	rows: RackRow[],
	isOpen: (id: TargetId, trouble: number) => boolean
): RackRow[] {
	const out: RackRow[] = [];
	// Depth of the nearest closed parent: the rows deeper than it are hidden.
	let closedAt = Infinity;
	for (let i = 0; i < rows.length; i += 1) {
		const row = rows[i];
		if (row.depth > closedAt) continue;
		closedAt = Infinity;
		let childCount = 0;
		let trouble = 0;
		for (let j = i + 1; j < rows.length && rows[j].depth > row.depth; j += 1) {
			if (rows[j].depth === row.depth + 1) childCount += 1;
			if (!rows[j].context && needsAttention(rows[j].state)) trouble += 1;
		}
		const open = childCount > 0 && isOpen(row.target.id, trouble);
		out.push({ ...row, childCount, trouble, open });
		if (childCount > 0 && !open) closedAt = row.depth;
	}
	return out;
}

/**
 * A separator between a parent's name and a child's own part: a mark with
 * spaces after it ("NAS70 · Databases", "nas - Docker", "pve: backup"), or
 * plain spaces ("nas Docker"). A mark glued to the next word ("nas-01") is
 * part of a name, not a separator.
 */
const PARENT_SEPARATOR = /^(?:\s*[·•|:/>›–—-]\s+|\s+(?![·•|:/>›–—-]))(?=\S)/u;

/**
 * The name of a sub-monitor as shown right under its parent: without the
 * parent's name it starts with ("NAS70 · Databases" under "NAS70" reads
 * "Databases"), since the row above already says it. The whole name stays
 * everywhere else (title, screen readers, search); a name that does not
 * start with the parent's, or that would be left empty, is kept whole.
 */
export function ownName(name: string, parentName: string | null | undefined): string {
	const parent = parentName?.trim() ?? '';
	if (!parent || name.length <= parent.length) return name;
	if (name.slice(0, parent.length).toLowerCase() !== parent.toLowerCase()) return name;
	const rest = name.slice(parent.length);
	const separator = PARENT_SEPARATOR.exec(rest);
	if (!separator) return name;
	const own = rest.slice(separator[0].length).trim();
	return own || name;
}

const STORAGE_KEY = 'dumbmonit-targets-open';

/** Parents opened or closed by hand: id → open. */
export function loadOpenChoices(): Map<TargetId, boolean> {
	const out = new Map<TargetId, boolean>();
	try {
		const raw = localStorage.getItem(STORAGE_KEY);
		const parsed: unknown = raw ? JSON.parse(raw) : null;
		if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
			for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
				const id = Number(key);
				if (Number.isInteger(id) && typeof value === 'boolean') out.set(id, value);
			}
		}
	} catch {
		// Nothing remembered: every parent starts at its default.
	}
	return out;
}

export function saveOpenChoices(choices: ReadonlyMap<TargetId, boolean>): void {
	try {
		if (choices.size === 0) localStorage.removeItem(STORAGE_KEY);
		else localStorage.setItem(STORAGE_KEY, JSON.stringify(Object.fromEntries(choices)));
	} catch {
		// Not remembered: the next visit opens every parent at its default.
	}
}
