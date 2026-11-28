/**
 * Topology map: which links shape the hierarchy, what is down, what it takes
 * down with it, where every node sits. Pure functions (no DOM, no Svelte), so
 * the same result feeds the SVG map, the list view and the exports.
 *
 * The map reads top down, like a network diagram: a declared parent above
 * what depends on it, a host above its services, a hypervisor above its
 * guests, a relay agent above what it polls. Every node keeps one *tree* link
 * — the surest one, declared before deduced — that places it; any other link
 * it has is drawn as a mesh line. A hub with many leaves lays them out as a
 * grid under it rather than one endless row.
 *
 * Remote sites are islands beside the main site, found without configuration
 * (the server's `site`: a `site` tag, an agent's declaration, or the relay
 * agent that polls the device), joined to it by a WAN link. Folders only
 * frame nodes when asked to (`byGroup`); likewise subnets (`bySubnet`): the
 * site's firewalls sit in a row on top and every other node goes in a light
 * frame for its subnet (`192.168.10.0/24 · VLAN 10 Servers`), a guest staying
 * in its hypervisor's. A link between sites joins the facing edges of their
 * frames, never crossing a node.
 *
 * Placement is deterministic: a refresh that changes states never moves a node.
 */
import type { TopologyEdgeTraffic, TopologyOrigin, TopologySiteSource, TopologySource } from '#lib/api/index.js';
import type { TargetState } from '#lib/format.js';

/** A device's state, or a guest's power state (a stopped VM is not an outage). */
export type NodeState = TargetState | 'running' | 'stopped';

export interface MapNode {
	id: string;
	targetId: number | null;
	/** Guest only: the hypervisor's target id (where its page lives). */
	hostId: number | null;
	name: string;
	kind: string;
	group: string;
	/** Remote site; `null` for the main site. */
	site: string | null;
	siteSource: TopologySiteSource | null;
	state: NodeState;
	/** Subnet (CIDR) the device sits in, when known. */
	subnet?: string | null;
	vlan?: number | null;
	vlanName?: string | null;
	/** Guest: the addresses its hypervisor reports. */
	ips?: string[];
	/** Grouping corrected on the map. */
	groupSource?: 'manual' | null;
}

export interface MapEdge {
	from: string;
	to: string;
	source: TopologySource;
	/** Deduced by the server (default), corrected by hand, or set by a learned rule. */
	origin?: TopologyOrigin;
	correctionId?: number | null;
	ruleId?: number | null;
	/** Switch ports at each end (`lldp` and `l2` links). */
	fromPort?: string | null;
	toPort?: string | null;
	/** Weathermap: the port's traffic, when the server knows it. */
	traffic?: TopologyEdgeTraffic | null;
}

/** The key of a link, as the map and its panels name it. */
export function edgeKey(edge: { from: string; to: string }): string {
	return `${edge.from}>${edge.to}`;
}

/**
 * Declared links first: they are what alert suppression follows. A node
 * placed by hand (`manual`) hangs where it was put, before anything deduced.
 */
const PRIORITY: Record<TopologySource, number> = { manual: -1, parent: 0, service: 1, hypervisor: 2, hosted: 3, relay: 4, samehost: 5, backup: 6, firewall: 7, lldp: 8, l2: 9 };

/** Links an outage travels along (a service runs on its guest). LLDP says who is cabled to whom, not who depends on whom. */
const PROPAGATES: ReadonlySet<TopologySource> = new Set(['parent', 'service', 'relay', 'hypervisor', 'hosted']);

export function isDown(state: NodeState): boolean {
	return state === 'offline' || state === 'down';
}

const ATTENTION: Record<NodeState, number> = {
	offline: 0,
	down: 0,
	misconfigured: 1,
	pending: 2,
	unknown: 2,
	online: 3,
	running: 3,
	stopped: 4,
	disabled: 4
};

const NETWORK_KINDS = new Set(['snmp', 'opnsense', 'pfsense', 'unifi', 'mikrotik', 'omada', 'pack.meraki', 'openwrt', 'fortigate', 'sophos', 'pack.paloalto', 'pack.sonicwall']);
/** Firewalls and gateways: the top of a site. */
const GATEWAY_KINDS = new Set(['opnsense', 'pfsense', 'fortigate', 'sophos', 'pack.paloalto', 'pack.sonicwall', 'mikrotik']);

export function isGateway(kind: string): boolean {
	return GATEWAY_KINDS.has(kind);
}

/** A virtual machine or container (a node of a Proxmox cluster is `pve_node`). */
export function isGuestKind(kind: string): boolean {
	return kind === 'vm' || kind === 'container';
}
/** Checks of one endpoint rather than of a machine (kept here: this module imports nothing at run time). */
const ENDPOINT_KINDS = new Set(['http', 'http_steps', 'tcp', 'udp', 'dns', 'tls', 'websocket', 'push', 'domain', 'provider', 'mail_blacklist', 'ip_blacklist', 'webchange', 'smtp', 'ssh', 'imap', 'pop3', 'agent_service', 'snmp_oid', 'radius', 'grpc']);
const PLATFORM_KINDS = new Set(['proxmox', 'pdm', 'xcpng', 'nutanix', 'vsphere', 'hyperv', 'kubernetes', 'truenas', 'synology', 'pbs', 'redfish', 'nut', 'libvirt']);

/** Where a device sits in a network diagram: gear first, then platforms, machines, services. */
export function tier(kind: string): number {
	if (NETWORK_KINDS.has(kind)) return 0;
	if (PLATFORM_KINDS.has(kind)) return 1;
	if (kind === 'agent') return 2;
	if (ENDPOINT_KINDS.has(kind)) return 4;
	return 3;
}

function compareNodes(a: MapNode, b: MapNode): number {
	return (
		tier(a.kind) - tier(b.kind) ||
		ATTENTION[a.state] - ATTENTION[b.state] ||
		a.name.localeCompare(b.name, 'en', { sensitivity: 'base', numeric: true })
	);
}

export interface Graph {
	nodes: Map<string, MapNode>;
	/** Links between known nodes, each pair once. */
	edges: MapEdge[];
	/** Tree parent of each node that has one (never closing a cycle). */
	parent: Map<string, string>;
	treeSource: Map<string, TopologySource>;
	/** The link that places each node with a tree parent. */
	treeEdge: Map<string, MapEdge>;
	children: Map<string, string[]>;
	/** Along the links an outage follows: who a node depends on, who depends on it. */
	upstream: Map<string, string[]>;
	downstream: Map<string, string[]>;
	/** LLDP neighbours, both ways. */
	cabled: Map<string, string[]>;
	down: Set<string>;
	/** Not down itself, but behind something that is: its alerts are suppressed. */
	impacted: Set<string>;
}

function push(map: Map<string, string[]>, key: string, value: string) {
	const list = map.get(key);
	if (list) list.push(value);
	else map.set(key, [value]);
}

/** Keeps the edges between known nodes, picks each node's tree parent and works out what an outage takes down. */
export function buildGraph(nodes: MapNode[], edges: MapEdge[]): Graph {
	const byId = new Map(nodes.map((n) => [n.id, n]));
	const kept = edges.filter((e) => e.from !== e.to && byId.has(e.from) && byId.has(e.to));

	const parent = new Map<string, string>();
	const treeSource = new Map<string, TopologySource>();
	const treeEdge = new Map<string, MapEdge>();
	const reachesUp = (from: string, target: string): boolean => {
		const seen = new Set<string>();
		for (let at: string | undefined = from; at !== undefined; at = parent.get(at)) {
			if (at === target) return true;
			if (seen.has(at)) return false;
			seen.add(at);
		}
		return false;
	};
	const ordered = kept
		.map((edge, index) => ({ edge, index }))
		.sort((a, b) => PRIORITY[a.edge.source] - PRIORITY[b.edge.source] || a.index - b.index);
	for (const { edge } of ordered) {
		if (parent.has(edge.to) || reachesUp(edge.from, edge.to)) continue;
		parent.set(edge.to, edge.from);
		treeSource.set(edge.to, edge.source);
		treeEdge.set(edge.to, edge);
	}

	const children = new Map<string, string[]>();
	for (const [child, up] of parent) push(children, up, child);
	for (const list of children.values()) list.sort((a, b) => compareNodes(byId.get(a)!, byId.get(b)!));

	const upstream = new Map<string, string[]>();
	const downstream = new Map<string, string[]>();
	const cabled = new Map<string, string[]>();
	for (const edge of kept) {
		if (PROPAGATES.has(edge.source)) {
			push(downstream, edge.from, edge.to);
			push(upstream, edge.to, edge.from);
		} else if (edge.source !== 'firewall') {
			// A firewall serving a subnet is not a cable: lighting up every device behind it says nothing.
			push(cabled, edge.from, edge.to);
			push(cabled, edge.to, edge.from);
		}
	}

	const down = new Set(nodes.filter((n) => isDown(n.state)).map((n) => n.id));
	const impacted = new Set<string>();
	const queue = [...down];
	for (let i = 0; i < queue.length; i++) {
		for (const next of downstream.get(queue[i]) ?? []) {
			if (down.has(next) || impacted.has(next)) continue;
			impacted.add(next);
			queue.push(next);
		}
	}

	return { nodes: byId, edges: kept, parent, treeSource, treeEdge, children, upstream, downstream, cabled, down, impacted };
}

function walk(map: Map<string, string[]>, from: string): Set<string> {
	const seen = new Set<string>();
	const queue = [from];
	for (let i = 0; i < queue.length; i++) {
		for (const next of map.get(queue[i]) ?? []) {
			if (next === from || seen.has(next)) continue;
			seen.add(next);
			queue.push(next);
		}
	}
	return seen;
}

/** Everything a node depends on, nearest first. */
export function ancestors(graph: Graph, id: string): string[] {
	return [...walk(graph.upstream, id)];
}

/** Everything that depends on a node. */
export function descendants(graph: Graph, id: string): string[] {
	return [...walk(graph.downstream, id)];
}

/** The dependency chain of a node (ancestors, descendants) plus its LLDP neighbours: what the map lights up. */
export function chainOf(graph: Graph, id: string): Set<string> {
	const set = new Set<string>([id, ...walk(graph.upstream, id), ...walk(graph.downstream, id)]);
	for (const n of graph.cabled.get(id) ?? []) set.add(n);
	return set;
}

/** The down devices an impacted node sits behind. */
export function blockers(graph: Graph, id: string): string[] {
	return ancestors(graph, id).filter((a) => graph.down.has(a));
}

// --- Sites -------------------------------------------------------------------

export interface SiteInfo {
	/** `''` for the main site. */
	key: string;
	name: string | null;
	source: TopologySiteSource | null;
	ids: string[];
}

/**
 * Nodes by site, the main site first. Nodes with no site are the main site;
 * when every node has one, the biggest site plays that part.
 */
export function splitSites(graph: Graph): SiteInfo[] {
	const sites = new Map<string, SiteInfo>();
	for (const node of graph.nodes.values()) {
		const key = node.site ?? '';
		let site = sites.get(key);
		if (!site) {
			site = { key, name: node.site, source: node.siteSource, ids: [] };
			sites.set(key, site);
		}
		site.ids.push(node.id);
	}
	const list = [...sites.values()];
	const main = sites.get('') ?? list.reduce((a, b) => (b.ids.length > a.ids.length ? b : a), list[0]);
	const rest = list
		.filter((s) => s !== main)
		.sort((a, b) => b.ids.length - a.ids.length || (a.name ?? '').localeCompare(b.name ?? '', 'en', { sensitivity: 'base' }));
	return main ? [main, ...rest] : rest;
}

// --- Layout ------------------------------------------------------------------
//
// A tidy tree drawn as an organisation chart. Every block is a box measured
// bottom up: a node over its children, the children side by side under a
// horizontal bus with a short drop into each one. Many leaves under one node
// make a compact grid (rows of `LEAF_COLS`), each extra row fed by its own bus
// from a trunk down the grid's left lane; siblings too wide for one row wrap
// the same way. Links are orthogonal with rounded corners, so the tree lines
// of a site never cross each other.
//
// Boxes are then packed bottom-left on a skyline (trees and loose nodes in a
// site, folders and subnets, sites on the map): each box drops into the
// lowest free spot of a band, the band chosen for the most compact landscape
// picture, so a small site sits beside a big one rather than above a hole.

/** Radius of a node with something under it, and of a leaf. */
export const R_HUB = 22;
export const R_LEAF = 17;
/** Horizontal pitch of a node, label included. */
const COL = 120;
/** Height of a node with its name and state under it. */
const NODE_H = R_HUB * 2 + 40;
/** From the top of a parent to the top of its children. */
const LEVEL = NODE_H + 40;
/** A bus runs this far above the row it feeds. */
const BUS_DROP = 22;
/** Pitch of the rows of a leaf grid: a node, its caption, the bus of the next row. */
const LEAF_ROW = NODE_H + 30;
/** At most this many leaves side by side under one node. */
const LEAF_COLS = 8;
/** Between two sibling subtrees. */
const PART_GAP = 16;
/** Siblings wider than this wrap into rows. */
const MAX_ROW = 1400;
/** Between two wrapped rows of siblings (their bus runs in it). */
const ROW_GAP = 40;
/** Left lane of a block with several rows: the trunk that feeds the rows under the first. */
const LANE = 28;
/** Between packed blocks: trees of a site, folders, subnets. */
const PACK_GAP = 40;
const FRAME_PAD = 22;
const FRAME_HEADER = 40;
/** A site frame's header holds a title with its source under it. */
const FRAME_HEADER_SITE = 58;
/** A site frame never gets narrower than its header needs. */
const FRAME_MIN_WIDTH = 270;
const SITE_GAP = 64;
/** Width over height the packing aims for: a landscape screen or sheet. */
const RATIO = 1.7;
/** Radius of the rounded corners of a tree link. */
const CORNER = 8;

export interface PlacedNode {
	node: MapNode;
	/** Centre of the circle. */
	x: number;
	y: number;
	r: number;
	down: boolean;
	impacted: boolean;
}

export type EdgeStyle = 'dependency' | 'mesh';

export interface PlacedEdge {
	key: string;
	from: string;
	to: string;
	/** `wan` for the synthetic link between two sites. */
	source: TopologySource | 'wan';
	style: EdgeStyle;
	/** Joins two sites. */
	wan: boolean;
	/** Leads from a down or impacted node: the outage travels along it. */
	impacted: boolean;
	path: string;
	/** Arrow head at the dependent end (`points` of a polygon), for dependencies. */
	arrow: string | null;
	/** Middle of the link's own stretch: where a link corrected by hand wears its mark. */
	mid: { x: number; y: number };
	origin: TopologyOrigin;
	/** Switch ports at each end, shown on hover and in the link panel. */
	fromPort?: string | null;
	toPort?: string | null;
	/** Weathermap: the port's traffic, when the server knows it. */
	traffic?: TopologyEdgeTraffic | null;
	/** An end of the link is down or behind an outage: its figures are stale, the link is drawn greyed out. */
	stale?: boolean;
}

export interface Frame {
	key: string;
	kind: 'site' | 'main' | 'group' | 'zone';
	/** A zone's subnet (`null`: unknown network), a folder or a site name. */
	label: string | null;
	/** Zone only: the VLAN of the subnet and its name. */
	vlan?: number | null;
	vlanName?: string | null;
	source: TopologySiteSource | null;
	x: number;
	y: number;
	width: number;
	height: number;
	count: number;
	down: number;
}

export interface MapLayout {
	nodes: PlacedNode[];
	edges: PlacedEdge[];
	frames: Frame[];
	width: number;
	height: number;
}

interface Pt {
	x: number;
	y: number;
}

/** How a node is reached from the node it hangs under on the map: corner points, centre to centre. */
interface Route {
	parent: string;
	points: Pt[];
}

interface Block {
	w: number;
	h: number;
	/** Centre of each node. */
	at: Map<string, Pt>;
	frames: Frame[];
	routes: Map<string, Route>;
	/**
	 * The nodes a parent above this block links to, each with its way in from
	 * the bus over the block (`y = -BUS_DROP`) down to its centre.
	 */
	entries: Map<string, Pt[]>;
}

function empty(): Block {
	return { w: 0, h: 0, at: new Map(), frames: [], routes: new Map(), entries: new Map() };
}

const shift = (points: Pt[], dx: number, dy: number): Pt[] => points.map((p) => ({ x: p.x + dx, y: p.y + dy }));

function paste(into: Block, block: Block, dx: number, dy: number) {
	for (const [id, spot] of block.at) into.at.set(id, { x: spot.x + dx, y: spot.y + dy });
	for (const frame of block.frames) into.frames.push({ ...frame, x: frame.x + dx, y: frame.y + dy });
	for (const [id, route] of block.routes) into.routes.set(id, { parent: route.parent, points: shift(route.points, dx, dy) });
}

/** Drops repeated points and the middle of straight runs. */
function simplify(points: Pt[]): Pt[] {
	const out: Pt[] = [];
	for (const p of points) {
		const last = out[out.length - 1];
		if (last && Math.abs(last.x - p.x) < 0.01 && Math.abs(last.y - p.y) < 0.01) continue;
		const before = out[out.length - 2];
		if (before && last) {
			const vertical = Math.abs(before.x - last.x) < 0.01 && Math.abs(last.x - p.x) < 0.01;
			const horizontal = Math.abs(before.y - last.y) < 0.01 && Math.abs(last.y - p.y) < 0.01;
			if (vertical || horizontal) out.pop();
		}
		out.push(p);
	}
	return out;
}

function single(id: string): Block {
	const block = empty();
	block.at.set(id, { x: COL / 2, y: R_HUB });
	block.entries.set(id, [
		{ x: COL / 2, y: -BUS_DROP },
		{ x: COL / 2, y: R_HUB }
	]);
	block.w = COL;
	block.h = NODE_H;
	return block;
}

/** Leaves in balanced rows of at most `LEAF_COLS`; the rows under the first are fed from the left lane. */
function leafGrid(ids: string[]): Block {
	const block = empty();
	const rows = Math.ceil(ids.length / LEAF_COLS);
	const cols = Math.ceil(ids.length / rows);
	const lane = rows > 1 ? LANE : 0;
	ids.forEach((id, i) => {
		const row = Math.floor(i / cols);
		const x = lane + (i % cols) * COL + COL / 2;
		const y = row * LEAF_ROW + R_HUB;
		block.at.set(id, { x, y });
		const bus = row * LEAF_ROW - BUS_DROP;
		block.entries.set(
			id,
			row === 0
				? [
						{ x, y: -BUS_DROP },
						{ x, y }
					]
				: [
						{ x: LANE / 2, y: -BUS_DROP },
						{ x: LANE / 2, y: bus },
						{ x, y: bus },
						{ x, y }
					]
		);
	});
	block.w = lane + cols * COL;
	block.h = (rows - 1) * LEAF_ROW + NODE_H;
	return block;
}

type KidsOf = (id: string) => string[];

/** A node over its children, linked by a bus; see the section comment. */
function subtree(id: string, kidsOf: KidsOf): Block {
	const kids = kidsOf(id);
	if (kids.length === 0) return single(id);

	// Few children: each its own column, in the map's order (an outage first).
	// Many: the ones with children of their own, and the leaves in one grid
	// standing where the first leaf would.
	const parts: Block[] = [];
	if (kids.length <= LEAF_COLS) {
		for (const k of kids) parts.push(subtree(k, kidsOf));
	} else {
		const leaves = kids.filter((k) => kidsOf(k).length === 0);
		const firstLeaf = kids.indexOf(leaves[0]);
		kids.forEach((k, i) => {
			if (i === firstLeaf && leaves.length > 0) parts.push(leafGrid(leaves));
			if (kidsOf(k).length > 0) parts.push(subtree(k, kidsOf));
		});
	}

	const rows: Block[][] = [];
	let rowWidth = 0;
	for (const part of parts) {
		const row = rows[rows.length - 1];
		if (row && rowWidth + PART_GAP + part.w <= MAX_ROW) {
			row.push(part);
			rowWidth += PART_GAP + part.w;
		} else {
			rows.push([part]);
			rowWidth = part.w;
		}
	}
	const lane = rows.length > 1 ? LANE : 0;

	const body = empty();
	const entries: { id: string; row: number; points: Pt[] }[] = [];
	const rowTops: number[] = [];
	let top = LEVEL;
	let contentW = 0;
	rows.forEach((row, k) => {
		rowTops.push(top);
		let x = lane;
		for (const part of row) {
			paste(body, part, x, top);
			for (const [child, points] of part.entries) entries.push({ id: child, row: k, points: shift(points, x, top) });
			x += part.w + PART_GAP;
		}
		contentW = Math.max(contentW, x - PART_GAP);
		top += Math.max(...row.map((p) => p.h)) + ROW_GAP;
	});

	// The parent sits over the middle of its first row.
	// (A grid's later rows, reached through its lane, do not pull it off centre.)
	const direct = entries.filter((e) => e.row === 0 && e.points.length === 2);
	const firstRow = (direct.length > 0 ? direct : entries.filter((e) => e.row === 0)).map((e) => e.points[0].x);
	const centre = (Math.min(...firstRow) + Math.max(...firstRow)) / 2;
	const dx = Math.max(0, COL / 2 - centre);
	const px = centre + dx;
	const block = empty();
	paste(block, body, dx, 0);
	block.at.set(id, { x: px, y: R_HUB });
	block.w = Math.max(contentW + dx, px + COL / 2);
	block.h = top - ROW_GAP;
	const bus = LEVEL - BUS_DROP;
	const laneX = dx + LANE / 2;
	for (const entry of entries) {
		const down: Pt[] = [
			{ x: px, y: R_HUB },
			{ x: px, y: bus }
		];
		if (entry.row > 0) down.push({ x: laneX, y: bus }, { x: laneX, y: rowTops[entry.row] - BUS_DROP });
		block.routes.set(entry.id, { parent: id, points: simplify([...down, ...shift(entry.points, dx, 0)]) });
	}
	block.entries.set(id, [
		{ x: px, y: -BUS_DROP },
		{ x: px, y: R_HUB }
	]);
	return block;
}

interface Segment {
	x: number;
	w: number;
	y: number;
}

/** The highest point of the skyline over `[x, x + w)`. */
function skylineAt(sky: Segment[], x: number, w: number): number {
	let y = 0;
	for (const s of sky) if (s.x < x + w && s.x + s.w > x) y = Math.max(y, s.y);
	return y;
}

/** Raises the skyline to `y` over `[x, x + w)`. */
function raise(sky: Segment[], x: number, w: number, y: number): Segment[] {
	const out: Segment[] = [];
	for (const s of sky) {
		const end = s.x + s.w;
		if (end <= x || s.x >= x + w) {
			out.push(s);
			continue;
		}
		if (s.x < x) out.push({ x: s.x, w: x - s.x, y: s.y });
		if (end > x + w) out.push({ x: x + w, w: end - (x + w), y: s.y });
	}
	out.push({ x, w, y });
	out.sort((a, b) => a.x - b.x);
	const merged: Segment[] = [];
	for (const s of out) {
		const last = merged[merged.length - 1];
		if (last && last.y === s.y && Math.abs(last.x + last.w - s.x) < 0.01) last.w += s.w;
		else merged.push({ ...s });
	}
	return merged;
}

/** Blocks in order, each in the lowest free spot (then the leftmost) of a band `band` wide, `gap` apart. */
function packInto(blocks: Block[], gap: number, band: number): Block {
	const out = empty();
	let sky: Segment[] = [{ x: 0, w: Infinity, y: 0 }];
	for (const block of blocks) {
		// The left edge always fits, even for a block wider than the band.
		let at: Pt = { x: 0, y: skylineAt(sky, 0, block.w + gap) };
		for (const s of sky) {
			if (s.x === 0 || s.x + block.w > band) continue;
			const y = skylineAt(sky, s.x, block.w + gap);
			if (y < at.y - 0.5) at = { x: s.x, y };
		}
		paste(out, block, at.x, at.y);
		sky = raise(sky, at.x, block.w + gap, at.y + block.h + gap);
		out.w = Math.max(out.w, at.x + block.w);
		out.h = Math.max(out.h, at.y + block.h);
	}
	return out;
}

/**
 * Blocks packed on a skyline (see the section comment). The band is the one,
 * among a few natural widths (the widest block, the blocks side by side
 * one more at a time, the square root of their area), that gives the
 * smallest picture close to a landscape screen: no hole beside a short block.
 */
function pack(blocks: Block[], gap: number): Block {
	if (blocks.length === 0) return empty();
	const widest = Math.max(...blocks.map((b) => b.w));
	const area = blocks.reduce((sum, b) => sum + (b.w + gap) * (b.h + gap), 0);
	const bands = new Set<number>([widest, Math.max(widest, Math.sqrt(area * RATIO))]);
	const runs: number[] = [];
	let run = -gap;
	for (const block of blocks) {
		run += gap + block.w;
		if (run > widest) runs.push(run);
	}
	// Hundreds of trees: ten of those widths are plenty, and keep it quick.
	const tries = Math.min(runs.length, 10);
	for (let i = 0; i < tries; i++) bands.add(runs[tries === 1 ? 0 : Math.round((i * (runs.length - 1)) / (tries - 1))]);
	let best: Block | null = null;
	let bestScore = Infinity;
	for (const band of [...bands].sort((a, b) => a - b)) {
		const out = packInto(blocks, gap, band);
		const ratio = out.w / Math.max(1, out.h);
		const score = out.w * out.h * Math.sqrt(Math.max(ratio / RATIO, RATIO / ratio));
		if (score < bestScore - 0.5) {
			best = out;
			bestScore = score;
		}
	}
	return best ?? packInto(blocks, gap, widest);
}

/** Nodes in rows of `cols`, no links between them. */
function plainGrid(ids: string[], cols: number): Block {
	const block = empty();
	ids.forEach((id, i) => block.at.set(id, { x: (i % cols) * COL + COL / 2, y: Math.floor(i / cols) * NODE_H + R_HUB }));
	block.w = Math.min(ids.length, cols) * COL;
	block.h = Math.ceil(ids.length / cols) * NODE_H;
	return block;
}

/**
 * A set of nodes as trees, the loose ones (no link inside the set) in a
 * compact grid. A node whose tree parent is elsewhere (another site, another
 * frame) but that has some other link inside the set hangs under that
 * neighbour rather than sitting alone: near what it is linked to, never
 * lost at the bottom.
 */
function cluster(graph: Graph, ids: string[]): Block {
	const members = new Set(ids);
	const up = new Map<string, string>();
	for (const id of ids) {
		const parent = graph.parent.get(id);
		if (parent !== undefined && members.has(parent)) up.set(id, parent);
	}
	const kids = new Map<string, string[]>();
	for (const [child, parent] of up) push(kids, parent, child);
	const byOrder = (a: string, b: string) => compareNodes(graph.nodes.get(a)!, graph.nodes.get(b)!);

	// Adopt a lone node under its surest neighbour in the set (never its own descendant).
	const climbs = (from: string, target: string) => {
		for (let at: string | undefined = from; at !== undefined; at = up.get(at)) if (at === target) return true;
		return false;
	};
	const neighbours = new Map<string, string[]>();
	graph.edges
		.map((edge, index) => ({ edge, index }))
		.filter(({ edge }) => members.has(edge.from) && members.has(edge.to))
		.sort((a, b) => PRIORITY[a.edge.source] - PRIORITY[b.edge.source] || a.index - b.index)
		.forEach(({ edge }) => {
			push(neighbours, edge.from, edge.to);
			push(neighbours, edge.to, edge.from);
		});
	for (const id of [...ids].sort(byOrder)) {
		if (up.has(id) || kids.has(id) || isGateway(graph.nodes.get(id)!.kind)) continue;
		const other = (neighbours.get(id) ?? []).find((n) => !climbs(n, id));
		if (other === undefined) continue;
		up.set(id, other);
		push(kids, other, id);
	}
	for (const list of kids.values()) list.sort(byOrder);
	const kidsOf: KidsOf = (id) => kids.get(id) ?? [];

	const roots = ids.filter((id) => !up.has(id)).sort(byOrder);
	const trees = roots.filter((id) => kidsOf(id).length > 0).map((id) => subtree(id, kidsOf));
	const idle = roots.filter((id) => kidsOf(id).length === 0);
	// A firewall with no one behind it (its high-availability twin) stays on top, beside the other one.
	const twins = idle.filter((id) => isGateway(graph.nodes.get(id)!.kind));
	const loose = idle.filter((id) => !twins.includes(id));
	const area = trees.reduce((sum, t) => sum + (t.w + PACK_GAP) * (t.h + PACK_GAP), 0) + idle.length * COL * NODE_H;
	const band = Math.max(COL, ...trees.map((t) => t.w), Math.sqrt(area * RATIO));
	const blocks: Block[] = [];
	if (twins.length > 0) blocks.push(plainGrid(twins, twins.length));
	blocks.push(...trees);
	if (loose.length > 0) {
		// Alone, the loose nodes make a landscape grid; beside trees, as wide as they leave room for.
		const alone = trees.length === 0 && twins.length === 0;
		const cols = alone ? Math.round(Math.sqrt((loose.length * RATIO * NODE_H) / COL)) : Math.floor((band + PACK_GAP) / COL);
		blocks.push(plainGrid(loose, Math.max(1, Math.min(loose.length, cols))));
	}
	return pack(blocks, PACK_GAP);
}

function framed(block: Block, frame: Omit<Frame, 'x' | 'y' | 'width' | 'height'>): Block {
	const out = empty();
	const light = frame.kind === 'group' || frame.kind === 'zone';
	const header = light ? FRAME_HEADER : FRAME_HEADER_SITE;
	out.w = Math.max(block.w + FRAME_PAD * 2, light ? 0 : FRAME_MIN_WIDTH);
	out.h = block.h + header + FRAME_PAD;
	out.frames.push({ ...frame, x: 0, y: 0, width: out.w, height: out.h });
	paste(out, block, (out.w - block.w) / 2, header);
	return out;
}

function counts(graph: Graph, ids: string[]) {
	return { count: ids.length, down: ids.filter((id) => graph.down.has(id)).length };
}

export type Grouping = 'none' | 'folder' | 'subnet';

/** One site: plain trees, or one framed block per folder or subnet. */
function siteBlock(graph: Graph, site: SiteInfo, grouping: Grouping): Block {
	if (grouping === 'subnet') return subnetBlock(graph, site);
	if (grouping === 'none') return cluster(graph, site.ids);
	const groups = new Map<string, string[]>();
	for (const id of site.ids) push(groups, graph.nodes.get(id)!.group, id);
	const blocks = [...groups]
		.sort(([a], [b]) => (a === '' ? 1 : b === '' ? -1 : a.localeCompare(b, 'en', { sensitivity: 'base' })))
		.map(([group, ids]) => framed(cluster(graph, ids), { key: `g:${site.key}:${group}`, kind: 'group', label: group || null, source: null, ...counts(graph, ids) }));
	return pack(blocks, PACK_GAP);
}

/** The subnet a node sits in: its own, else its hypervisor's (a guest has none). `''`: unknown. */
function subnetOf(graph: Graph, id: string): string {
	const seen = new Set<string>();
	for (let at: string | undefined = id; at !== undefined && !seen.has(at); at = graph.parent.get(at)) {
		seen.add(at);
		const node = graph.nodes.get(at)!;
		if (node.subnet) return node.subnet;
		if (!isGuestKind(node.kind) && node.kind !== 'pve_node') break;
	}
	return '';
}

/** Numeric sort key of `a.b.c.d/n` so zones read in address order. */
function subnetKey(subnet: string): number {
	const [ip] = subnet.split('/');
	return ip.split('.').reduce((sum, part) => sum * 256 + (Number(part) || 0), 0);
}

/** Firewalls in a row on top, then one light frame per subnet (the unknown network last). */
function subnetBlock(graph: Graph, site: SiteInfo): Block {
	const gateways = site.ids.filter((id) => isGateway(graph.nodes.get(id)!.kind));
	const zones = new Map<string, string[]>();
	for (const id of site.ids) {
		if (gateways.includes(id)) continue;
		push(zones, subnetOf(graph, id), id);
	}
	const blocks = [...zones]
		.sort(([a], [b]) => (a === '' ? 1 : b === '' ? -1 : subnetKey(a) - subnetKey(b)))
		.map(([subnet, ids]) => {
			// A zone is named after the VLAN any of its devices reports.
			const named = ids.map((id) => graph.nodes.get(id)!).find((n) => n.subnet === subnet && (n.vlan || n.vlanName));
			return framed(cluster(graph, ids), {
				key: `z:${site.key}:${subnet}`,
				kind: 'zone',
				label: subnet || null,
				vlan: named?.vlan ?? null,
				vlanName: named?.vlanName ?? null,
				source: null,
				...counts(graph, ids)
			});
		});
	const body = pack(blocks, PACK_GAP);
	if (gateways.length === 0) return body;
	const top = plainGrid(gateways.sort((a, b) => compareNodes(graph.nodes.get(a)!, graph.nodes.get(b)!)), Math.min(gateways.length, 4));
	if (blocks.length === 0) return top;
	const out = empty();
	out.w = Math.max(top.w, body.w);
	paste(out, top, (out.w - top.w) / 2, 0);
	paste(out, body, (out.w - body.w) / 2, top.h + PACK_GAP);
	out.h = top.h + PACK_GAP + body.h;
	return out;
}

const f = (n: number) => n.toFixed(1);

function toward(from: Pt, to: Pt, distance: number): Pt {
	const len = Math.hypot(to.x - from.x, to.y - from.y) || 1;
	return { x: from.x + ((to.x - from.x) / len) * distance, y: from.y + ((to.y - from.y) / len) * distance };
}

function arrowHead(tipX: number, tipY: number, fromX: number, fromY: number): string {
	const len = Math.hypot(tipX - fromX, tipY - fromY) || 1;
	const ux = (tipX - fromX) / len;
	const uy = (tipY - fromY) / len;
	const baseX = tipX - ux * 8;
	const baseY = tipY - uy * 8;
	return `${f(tipX)},${f(tipY)} ${f(baseX - uy * 4)},${f(baseY + ux * 4)} ${f(baseX + uy * 4)},${f(baseY - ux * 4)}`;
}

type Geometry = { path: string; arrow: string | null; mid: { x: number; y: number } };

/** A tree link along its route, from the rim of one circle to the rim of the other, corners rounded. */
function routedGeometry(route: Pt[], ra: number, rb: number, arrow: boolean): Geometry {
	const points = route.map((p) => ({ ...p }));
	const n = points.length;
	points[0] = toward(points[0], points[1], ra + 1);
	points[n - 1] = toward(points[n - 1], points[n - 2], rb + (arrow ? 3 : 1));
	let path = `M${f(points[0].x)},${f(points[0].y)}`;
	for (let i = 1; i < n - 1; i++) {
		const a = points[i - 1];
		const c = points[i];
		const b = points[i + 1];
		const r = Math.min(CORNER, Math.hypot(c.x - a.x, c.y - a.y) / 2, Math.hypot(b.x - c.x, b.y - c.y) / 2);
		const s = toward(c, a, r);
		const e = toward(c, b, r);
		path += ` L${f(s.x)},${f(s.y)} Q${f(c.x)},${f(c.y)} ${f(e.x)},${f(e.y)}`;
	}
	const end = points[n - 1];
	const before = points[n - 2];
	path += ` L${f(end.x)},${f(end.y)}`;
	return {
		path,
		arrow: arrow ? arrowHead(end.x, end.y, before.x, before.y) : null,
		// The last stretch is the link's own (the bus is shared with its siblings).
		mid: { x: (before.x + end.x) / 2, y: (before.y + end.y) / 2 }
	};
}

/**
 * Any other link inside a site: a soft curve, vertical tangents, so it reads
 * apart from the square tree lines. Two nodes on one row are joined by an arc
 * over them rather than a line through their neighbours.
 */
function meshGeometry(a: PlacedNode, b: PlacedNode, arrow: boolean): Geometry {
	const gap = arrow ? 3 : 1;
	if (Math.abs(b.y - a.y) < NODE_H / 2) {
		const y1 = a.y - a.r - 1;
		const y2 = b.y - b.r - gap;
		const top = Math.min(a.y - a.r, b.y - b.r) - (24 + Math.min(48, Math.abs(b.x - a.x) * 0.1));
		return {
			path: `M${f(a.x)},${f(y1)} C${f(a.x)},${f(top)} ${f(b.x)},${f(top)} ${f(b.x)},${f(y2)}`,
			arrow: arrow ? arrowHead(b.x, y2, b.x, top) : null,
			mid: { x: (a.x + b.x) / 2, y: (y1 + 6 * top + y2) / 8 }
		};
	}
	const dir = b.y > a.y ? 1 : -1;
	const y1 = a.y + dir * (a.r + 1);
	const y2 = b.y - dir * (b.r + gap);
	const mid = (y1 + y2) / 2;
	return {
		path: `M${f(a.x)},${f(y1)} C${f(a.x)},${f(mid)} ${f(b.x)},${f(mid)} ${f(b.x)},${f(y2)}`,
		arrow: arrow ? arrowHead(b.x, y2, b.x, mid) : null,
		mid: { x: (a.x + b.x) / 2, y: mid }
	};
}

/**
 * A link between two sites: from the edge of one frame to the facing edge of
 * the other, level with each end node where the edge allows, as one smooth
 * curve across the gap. It never enters a frame.
 */
function siteGeometry(fa: Frame, fb: Frame, a: Pt, b: Pt, arrow: boolean): Geometry | null {
	const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(lo, hi), Math.max(lo, v));
	const gapX = fb.x >= fa.x + fa.width ? fb.x - fa.x - fa.width : fa.x >= fb.x + fb.width ? fa.x - fb.x - fb.width : -1;
	const gapY = fb.y >= fa.y + fa.height ? fb.y - fa.y - fa.height : fa.y >= fb.y + fb.height ? fa.y - fb.y - fb.height : -1;
	if (gapX < 0 && gapY < 0) return null;
	let pa: Pt;
	let pb: Pt;
	let path: string;
	let from: Pt;
	if (gapX >= gapY) {
		const right = fb.x >= fa.x + fa.width;
		pa = { x: right ? fa.x + fa.width : fa.x, y: clamp(a.y, fa.y + FRAME_HEADER_SITE, fa.y + fa.height - FRAME_PAD) };
		pb = { x: right ? fb.x : fb.x + fb.width, y: clamp(b.y, fb.y + FRAME_HEADER_SITE, fb.y + fb.height - FRAME_PAD) };
		const h = (pb.x - pa.x) / 2;
		path = `M${f(pa.x)},${f(pa.y)} C${f(pa.x + h)},${f(pa.y)} ${f(pb.x - h)},${f(pb.y)} ${f(pb.x)},${f(pb.y)}`;
		from = { x: pb.x - h, y: pb.y };
	} else {
		const below = fb.y >= fa.y + fa.height;
		pa = { x: clamp(a.x, fa.x + FRAME_PAD * 2, fa.x + fa.width - FRAME_PAD * 2), y: below ? fa.y + fa.height : fa.y };
		pb = { x: clamp(b.x, fb.x + FRAME_PAD * 2, fb.x + fb.width - FRAME_PAD * 2), y: below ? fb.y : fb.y + fb.height };
		const v = (pb.y - pa.y) / 2;
		path = `M${f(pa.x)},${f(pa.y)} C${f(pa.x)},${f(pa.y + v)} ${f(pb.x)},${f(pb.y - v)} ${f(pb.x)},${f(pb.y)}`;
		from = { x: pb.x, y: pb.y - v };
	}
	return { path, arrow: arrow ? arrowHead(pb.x, pb.y, from.x, from.y) : null, mid: { x: (pa.x + pb.x) / 2, y: (pa.y + pb.y) / 2 } };
}

/** Positions every node, link and frame of the map. Deterministic: the same graph always gives the same picture. */
export function layoutMap(graph: Graph, options: { byGroup?: boolean; bySubnet?: boolean } = {}): MapLayout {
	const sites = splitSites(graph);
	if (sites.length === 0) return { nodes: [], edges: [], frames: [], width: 0, height: 0 };
	const islands = sites.length > 1;

	const grouping: Grouping = options.bySubnet ? 'subnet' : options.byGroup ? 'folder' : 'none';
	const blocks = sites.map((site, i) => {
		const inner = siteBlock(graph, site, grouping);
		if (!islands) return inner;
		return framed(inner, { key: `s:${site.key}`, kind: i === 0 ? 'main' : 'site', label: site.name, source: site.source, ...counts(graph, site.ids) });
	});
	// Sites flow side by side, the main one first, each as big as its content.
	const all = islands ? pack(blocks, SITE_GAP) : blocks[0];

	const hubs = new Set([...all.routes.values()].map((r) => r.parent));
	const nodes: PlacedNode[] = [];
	const placed = new Map<string, PlacedNode>();
	let width = 0;
	let height = 0;
	for (const [id, spot] of all.at) {
		const p: PlacedNode = {
			node: graph.nodes.get(id)!,
			x: spot.x,
			y: spot.y,
			r: hubs.has(id) || !all.routes.has(id) ? R_HUB : R_LEAF,
			down: graph.down.has(id),
			impacted: graph.impacted.has(id)
		};
		nodes.push(p);
		placed.set(id, p);
		width = Math.max(width, spot.x + COL / 2);
		height = Math.max(height, spot.y + NODE_H - R_HUB);
	}
	for (const frame of all.frames) {
		width = Math.max(width, frame.x + frame.width);
		height = Math.max(height, frame.y + frame.height);
	}

	const siteOf = (id: string) => graph.nodes.get(id)!.site ?? '';
	const frameOf = (key: string) => all.frames.find((fr) => fr.key === `s:${key}`);
	const edges: PlacedEdge[] = graph.edges
		.filter((e) => placed.has(e.from) && placed.has(e.to))
		.map((e) => {
			const a = placed.get(e.from)!;
			const b = placed.get(e.to)!;
			// A node attached by hand depends on its new parent, on the map at least: drawn the same way.
			const dependency = e.source === 'parent' || e.source === 'manual';
			const across = islands && siteOf(e.from) !== siteOf(e.to);
			let geometry: Geometry | null = null;
			if (across) {
				const fa = frameOf(siteOf(e.from));
				const fb = frameOf(siteOf(e.to));
				if (fa && fb) geometry = siteGeometry(fa, fb, a, b, dependency);
			} else {
				const down = all.routes.get(e.to);
				const upward = all.routes.get(e.from);
				if (down && down.parent === e.from) geometry = routedGeometry(down.points, a.r, b.r, dependency);
				else if (upward && upward.parent === e.to) geometry = routedGeometry([...upward.points].reverse(), a.r, b.r, dependency);
			}
			return {
				key: edgeKey(e),
				from: e.from,
				to: e.to,
				source: e.source,
				origin: e.origin ?? 'deduced',
				fromPort: e.fromPort ?? null,
				toPort: e.toPort ?? null,
				traffic: e.traffic ?? null,
				// The outage cascade of the dependencies (down, then impacted) greys out the cabling behind it too.
				stale: !!e.traffic && [e.from, e.to].some((id) => graph.down.has(id) || graph.impacted.has(id)),
				style: dependency ? ('dependency' as const) : ('mesh' as const),
				wan: across,
				impacted: PROPAGATES.has(e.source) && (graph.down.has(e.from) || graph.impacted.has(e.from)),
				...(geometry ?? meshGeometry(a, b, dependency))
			};
		});

	// A WAN link from the main site to every island no link reaches yet.
	if (islands) {
		const home = frameOf(sites[0].key)!;
		for (const site of sites.slice(1)) {
			if (edges.some((e) => e.wan && e.source !== 'wan' && (siteOf(e.from) === site.key || siteOf(e.to) === site.key))) continue;
			const island = frameOf(site.key)!;
			const centre = (fr: Frame): Pt => ({ x: fr.x + fr.width / 2, y: fr.y + FRAME_HEADER_SITE / 2 + 6 });
			const geometry = siteGeometry(home, island, centre(home), centre(island), false);
			if (!geometry) continue;
			const allDown = site.ids.length > 0 && site.ids.every((id) => graph.down.has(id) || graph.impacted.has(id));
			edges.push({ key: `wan:${site.key}`, from: '', to: '', source: 'wan', style: 'mesh', wan: true, impacted: allDown, origin: 'deduced', ...geometry });
		}
	}

	// Mesh under dependencies, the outage on top.
	edges.sort(
		(a, b) => Number(a.style === 'dependency') - Number(b.style === 'dependency') || Number(a.impacted) - Number(b.impacted)
	);
	return { nodes, edges, frames: all.frames, width, height };
}

// --- List view ---------------------------------------------------------------

/** One line of the list view: the same tree, flattened. */
export interface ListRow {
	node: MapNode;
	depth: number;
	down: boolean;
	impacted: boolean;
	/** How the node hangs from the row above it (`null` for a root). */
	via: TopologySource | null;
	/** Weathermap: traffic of the link that places the node, and that link's upstream end. */
	traffic: TopologyEdgeTraffic | null;
	linkFrom: string | null;
}

/** Depth-first rows of the tree, site by site (the main site first). */
export function listRows(graph: Graph): { site: SiteInfo; rows: ListRow[] }[] {
	return splitSites(graph).map((site) => {
		const members = new Set(site.ids);
		const rows: ListRow[] = [];
		const seen = new Set<string>();
		const visit = (id: string, depth: number) => {
			if (seen.has(id)) return;
			seen.add(id);
			rows.push({
				node: graph.nodes.get(id)!,
				depth,
				down: graph.down.has(id),
				impacted: graph.impacted.has(id),
				via: depth > 0 ? (graph.treeSource.get(id) ?? null) : null,
				traffic: depth > 0 ? (graph.treeEdge.get(id)?.traffic ?? null) : null,
				linkFrom: depth > 0 ? (graph.treeEdge.get(id)?.from ?? null) : null
			});
			for (const child of graph.children.get(id) ?? []) if (members.has(child)) visit(child, depth + 1);
		};
		site.ids
			.filter((id) => {
				const up = graph.parent.get(id);
				return up === undefined || !members.has(up);
			})
			.sort((a, b) => compareNodes(graph.nodes.get(a)!, graph.nodes.get(b)!))
			.forEach((id) => visit(id, 0));
		return { site, rows };
	});
}
