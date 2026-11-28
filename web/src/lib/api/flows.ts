/**
 * Network flows (beta): the NetFlow / IPFIX / sFlow receiver settings
 * (administrators) and the top talkers of one exporting device. Mirrors
 * `crates/server/src/api/flows.rs`. The device read can answer 502 when
 * VictoriaMetrics is unreachable.
 */
import { request } from './client';
import type { FlowOverview, FlowRange, FlowSettings, TargetFlows, TargetId } from './types';

export function getFlowSettings(signal?: AbortSignal): Promise<FlowOverview> {
	return request<FlowOverview>('/flows/settings', { signal });
}

export function setFlowSettings(settings: FlowSettings): Promise<FlowOverview> {
	return request<FlowOverview>('/flows/settings', { method: 'PUT', body: settings });
}

export function getTargetFlows(id: TargetId, range: FlowRange, signal?: AbortSignal): Promise<TargetFlows> {
	return request<TargetFlows>(`/targets/${id}/flows`, { query: { range }, signal });
}
