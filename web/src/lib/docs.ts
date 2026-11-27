/** The documentation is English only: every link points to the English pages. */
const DOCS_ROOT = 'https://dumbmonit.selfhostr.io/docs/';

/** Page "Website banner" of the documentation. */
export function websiteBannerDocsUrl(): string {
	return `${DOCS_ROOT}integrations/website-banner/`;
}

/** A page of the documentation (`devices/agent/`, `install/remote-site/#anchor`). */
export function docsUrl(page: string): string {
	return `${DOCS_ROOT}${page}`;
}

/** Kinds without a page of their own: the closest page of the documentation. */
const KIND_DOCS: Record<string, string> = {
	http: 'website',
	http_steps: 'http-scenario',
	snmp_oid: 'snmp',
	victorialogs: 'victoriametrics',
	agent_service: 'agent',
	agent_script: 'custom-scripts',
	dummy: ''
};
const SERVICE_KINDS = new Set(['dns', 'grpc', 'imap', 'mqtt', 'mysql', 'ntp', 'ping', 'pop3', 'postgres', 'radius', 'smtp', 'ssh', 'tcp', 'tls', 'udp', 'websocket']);

/** Page of the documentation that covers a monitor kind (`devices/proxmox/`). */
export function kindDocPage(kind: string): string {
	if (SERVICE_KINDS.has(kind)) return 'devices/services/';
	const slug = kind in KIND_DOCS ? KIND_DOCS[kind] : kind;
	return slug ? `devices/${slug}/` : 'devices/';
}
