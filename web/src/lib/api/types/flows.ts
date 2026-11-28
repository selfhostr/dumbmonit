/**
 * Network flows (beta): the receiver settings and the top talkers of one
 * device. Mirrors `crates/server/src/api/flows.rs` and `crates/server/src/flows`.
 */
import type { TargetId } from './targets.js';

export type FlowProtocol = 'netflow' | 'sflow';

export interface FlowSettings {
	enabled: boolean;
	/** NetFlow v5/v9 and IPFIX. */
	netflow_port: number;
	sflow_port: number;
}

export interface FlowListener {
	protocol: FlowProtocol;
	port: number;
	listening: boolean;
	error?: string | null;
}

export interface FlowCounters {
	datagrams: number;
	flows: number;
	dropped_unknown: number;
	dropped_malformed: number;
	missing_template: number;
	series_capped: number;
}

export interface FlowExporter {
	target_id: TargetId;
	target_name: string;
	address: string;
	protocol: FlowProtocol;
	datagrams: number;
	/** UTC without suffix. */
	last_seen: string;
}

export interface FlowUnknownSource {
	address: string;
	last_seen: string;
}

export interface FlowOverview {
	settings: FlowSettings;
	listeners: FlowListener[];
	counters: FlowCounters;
	exporters: FlowExporter[];
	unknown_sources: FlowUnknownSource[];
}

export type FlowRange = '1h' | '6h' | '24h' | '7d';

export interface FlowTalker {
	address: string;
	bytes: number;
}

export interface FlowConversation {
	src: string;
	dst: string;
	/** `tcp`, `udp`, `icmp`… or the IP protocol number. */
	proto: string;
	/** Lower of the two ports (usually the service); `0` without ports. */
	port: string;
	bytes: number;
	packets?: number | null;
}

export interface FlowPort {
	proto: string;
	port: string;
	bytes: number;
}

export interface TargetFlows {
	range: string;
	/** The device exported flows in the last 24 hours. */
	available: boolean;
	total_bytes: number;
	total_packets: number;
	talkers: FlowTalker[];
	conversations: FlowConversation[];
	ports: FlowPort[];
}
