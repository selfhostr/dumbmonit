import { request } from './client';

// --- Optional modules -------------------------------------------------------

/** Optional modules of the instance, all off on a fresh one. */
export interface Features {
	wall: boolean;
	sla_reports: boolean;
	status_pages: boolean;
	slo: boolean;
	threat_map: boolean;
}

/** Reads which optional modules are on. Anticipated: a server without the route answers 404. */
export function getFeatures(signal?: AbortSignal): Promise<Features> {
	return request<Features>('/settings/features', { signal, anticipated: true });
}

/** Turns modules on or off; keys left out keep their value. Returns what the server kept. */
export function updateFeatures(patch: Partial<Features>): Promise<Features> {
	return request<Features>('/settings/features', { method: 'PUT', body: patch });
}
