/**
 * Integration packs: device types declared in YAML. Mirrors
 * `crates/server/src/api/packs.rs`. Administrators only.
 *
 * The YAML travels in a `{"yaml": "…"}` envelope so the request helper keeps
 * its single JSON path (and its anti-CSRF header).
 */
import { request } from './client';
import type {
	PackInstallReport,
	PackView,
	TemplateImportApplied,
	TemplateImportPreview,
	TemplateImportRequest
} from './types';

/** Where packs are documented. The interface links to it, never serves it. */
export const PACKS_DOC_URL = 'https://dumbmonit.selfhostr.io/docs/packs/';

export function listPacks(signal?: AbortSignal): Promise<PackView[]> {
	return request<PackView[]>('/packs', { signal });
}

/** Installs a pack, or updates the one with the same id. A 400 lists every validation error. */
export function installPack(yaml: string): Promise<PackInstallReport> {
	return request<PackInstallReport>('/packs', { method: 'POST', body: { yaml } });
}

export function setPackEnabled(id: string, enabled: boolean): Promise<PackView> {
	return request<PackView>(`/packs/${encodeURIComponent(id)}/${enabled ? 'enable' : 'disable'}`, {
		method: 'PUT'
	});
}

/** Refused with a 409 while devices still use the pack. */
export function uninstallPack(id: string): Promise<void> {
	return request<void>(`/packs/${encodeURIComponent(id)}`, { method: 'DELETE' });
}

/** Largest file the template import accepts (the server checks it too). */
export const TEMPLATE_IMPORT_MAX_BYTES = 2 * 1024 * 1024;

/** Converts a Zabbix or snmp_exporter file into a pack, without installing it. */
export function previewTemplateImport(body: TemplateImportRequest): Promise<TemplateImportPreview> {
	return request<TemplateImportPreview>('/import/templates/preview', { method: 'POST', body });
}

/** Converts the same file again on the server and installs the resulting pack. */
export function applyTemplateImport(body: TemplateImportRequest): Promise<TemplateImportApplied> {
	return request<TemplateImportApplied>('/import/templates/apply', { method: 'POST', body });
}

/** Kinds added by a pack are named `pack.<id>`. */
export function isPackKind(kind: string): boolean {
	return kind.startsWith('pack.');
}

// --- Community registry (`crates/server/src/api/packs_registry.rs`) ---------

export type RegistryPackStatus = 'available' | 'installed' | 'update' | 'incompatible';

export interface RegistryPack {
	id: string;
	name: string;
	version: string;
	description: string;
	author: string;
	license: string;
	min_version: string | null;
	installed_version: string | null;
	status: RegistryPackStatus;
}

export interface RegistryView {
	url: string;
	configured: boolean;
	packs: RegistryPack[];
	skipped: string[];
	/** Why the index could not be read, in English as served; null when fine. */
	error: string | null;
}

export function getPackRegistry(signal?: AbortSignal): Promise<RegistryView> {
	return request<RegistryView>('/packs/registry', { signal });
}

/** An empty address turns the registry off. */
export function setPackRegistry(url: string): Promise<{ url: string }> {
	return request<{ url: string }>('/packs/registry/settings', { method: 'PUT', body: { url } });
}

export function installRegistryPack(id: string): Promise<PackInstallReport> {
	return request<PackInstallReport>('/packs/registry/install', { method: 'POST', body: { id } });
}

/** How to host a registry. */
export const PACK_REGISTRY_DOC_URL = 'https://dumbmonit.selfhostr.io/docs/packs/registry/';
