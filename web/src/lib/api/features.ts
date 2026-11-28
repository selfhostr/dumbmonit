/**
 * Optional modules (wall, SLA reports, status pages, SLO, threat map). Mirrors `crates/server/src/api/features.rs`.
 */
import { request } from './client';
import type { Features, FeaturesPatch } from './types';

/** Any signed-in account. */
export function getFeatures(signal?: AbortSignal): Promise<Features> {
	return request<Features>('/settings/features', { signal });
}

/** Administrators only: returns the whole object. */
export function updateFeatures(patch: FeaturesPatch): Promise<Features> {
	return request<Features>('/settings/features', { method: 'PUT', body: patch });
}
