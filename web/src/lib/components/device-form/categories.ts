/**
 * Categories of the "Add a monitor" picker, and the filters that narrow it.
 *
 * Pure data and predicates, no icon and no text: the titles and icons live
 * in `kinds.ts`, so this file can be tested alone. A kind the table below does
 * not know lands in `packs` (kinds named `pack.<id>`) or in `other`.
 */

/** Display order of the categories. `import` is not a kind: it is added by the picker, last. */
export const CATEGORY_IDS = [
	'machines',
	'network',
	'virtualization',
	'storage',
	'databases',
	'web',
	'checks',
	'mail',
	'observability',
	'security',
	'cloud',
	'apps',
	'media',
	'packs',
	'other'
] as const;

export type CategoryId = (typeof CATEGORY_IDS)[number];

/**
 * Choice ids per category, in display order. `agent:relay` and `agent:ad`
 * are the agent's own entries (relay, Active Directory rollout).
 */
export const CATEGORY_KINDS: Record<Exclude<CategoryId, 'packs' | 'other'>, string[]> = {
	machines: ['agent', 'agent:relay', 'agent:ad', 'winrm', 'redfish', 'nut'],
	network: ['snmp', 'netconfig', 'opnsense', 'pfsense', 'fortigate', 'sophos', 'paloalto', 'sonicwall', 'mikrotik', 'unifi', 'omada', 'meraki', 'arubacentral', 'openvpn', 'tailscale'],
	virtualization: ['proxmox', 'pdm', 'vsphere', 'xcpng', 'nutanix', 'kubernetes'],
	storage: ['synology', 'truenas', 'unraid', 'qnap', 'ceph', 'minio', 'pbs', 'veeam', 'kopia', 'duplicati'],
	databases: ['postgres', 'mysql', 'mssql', 'oracle', 'mongodb', 'redis', 'elasticsearch', 'influxdb', 'rabbitmq', 'kafka', 'mqtt'],
	web: ['http', 'website', 'webchange', 'tls', 'websocket', 'grpc', 'nginx', 'apache', 'traefik', 'caddy', 'npm'],
	checks: ['ping', 'tcp', 'udp', 'dns', 'ntp', 'ssh', 'push', 'snmp_oid', 'radius', 'ip_blacklist'],
	mail: ['smtp', 'imap', 'pop3', 'mailcow', 'mdaemon', 'securitygateway', 'pmg', 'mail_blacklist'],
	observability: ['victoriametrics', 'victorialogs', 'loki', 'graylog', 'prometheus', 'otlp'],
	security: ['activedirectory', 'keycloak', 'authentik', 'entra', 'vaultwarden', 'crowdsec', 'pihole', 'adguard'],
	cloud: ['m365', 'azure', 'aws', 'cloudflare', 'hetzner', 'ovh', 'infomaniak', 'domain', 'provider'],
	apps: ['nextcloud', 'immich', 'paperless', 'gitlab', 'forgejo'],
	media: ['jellyfin', 'plex', 'sonarr', 'radarr', 'lidarr', 'prowlarr', 'qbittorrent', 'homeassistant', 'unifiprotect', 'frigate']
};

const CATEGORY_OF = new Map<string, CategoryId>(
	Object.entries(CATEGORY_KINDS).flatMap(([category, ids]) => ids.map((id) => [id, category as CategoryId] as const))
);

/** The category of a choice (`id` is its kind, or `agent:<feature>`). */
export function categoryOf(id: string): CategoryId {
	return CATEGORY_OF.get(id) ?? (id.startsWith('pack.') ? 'packs' : 'other');
}

/** Position inside the category: the fixed order above, then the unknown ones. */
export function rankInCategory(id: string): number {
	const list = (CATEGORY_KINDS as Record<string, string[] | undefined>)[categoryOf(id)];
	const index = list ? list.indexOf(id) : -1;
	return index === -1 ? Number.MAX_SAFE_INTEGER : index;
}

/** Yes/no filters that combine with the categories and the text. */
export const FACETS = ['nologin', 'beta', 'pack'] as const;
export type Facet = (typeof FACETS)[number];

/** What the filters read on a choice. */
export interface Filterable {
	category: CategoryId;
	beta: boolean;
	pack: boolean;
	/** Needs no credential: a probe, or something read without logging in. */
	noLogin: boolean;
}

/** Whether the credential forms a kind accepts include one that asks for something. */
export function needsLogin(credentialTypes: string[]): boolean {
	return credentialTypes.length > 0 && !credentialTypes.includes('none');
}

export function facetMatches(choice: Filterable, facet: Facet): boolean {
	switch (facet) {
		case 'nologin':
			return choice.noLogin;
		case 'beta':
			return choice.beta;
		case 'pack':
			return choice.pack;
	}
}

/**
 * Categories are alternatives (any of the chosen ones); facets all have to
 * hold. Empty sets filter nothing.
 */
export function passesFilters(choice: Filterable, categories: ReadonlySet<string>, facets: ReadonlySet<Facet>): boolean {
	if (categories.size > 0 && !categories.has(choice.category)) return false;
	for (const facet of facets) if (!facetMatches(choice, facet)) return false;
	return true;
}
