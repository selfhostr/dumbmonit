/**
 * The short kind tag of a monitor in the table ("PostgreSQL", "HTTP",
 * "Docker"), beside its address. The full kind label stays its tooltip and
 * what the search and the kind filter read.
 *
 * Protocol and product names are the same in every language and are written
 * here; a tag made of words goes through Paraglide; any other kind falls back
 * to its full label (translated, or as the server describes it).
 */
import type { Target } from '#lib/api/index.js';
import { m } from '#lib/paraglide/messages.js';

/** Kinds whose full label is a sentence around a protocol or a product name. */
const NAME: Record<string, string> = {
	http: 'HTTP',
	postgres: 'PostgreSQL',
	mysql: 'MySQL',
	mssql: 'SQL Server',
	redis: 'Redis',
	ping: 'Ping',
	tcp: 'TCP',
	udp: 'UDP',
	dns: 'DNS',
	tls: 'TLS',
	smtp: 'SMTP',
	imap: 'IMAP',
	pop3: 'POP3',
	ssh: 'SSH',
	snmp: 'SNMP',
	snmp_oid: 'SNMP',
	ntp: 'NTP',
	mqtt: 'MQTT',
	grpc: 'gRPC',
	websocket: 'WebSocket',
	radius: 'RADIUS',
	winrm: 'Windows',
	hyperv: 'Hyper-V',
	redfish: 'Redfish',
	ipmi: 'IPMI',
	kubernetes: 'Kubernetes',
	gitlab: 'GitLab',
	forgejo: 'Forgejo',
	mikrotik: 'MikroTik',
	mdaemon: 'MDaemon',
	securitygateway: 'SecurityGateway',
	veeam: 'Veeam',
	vsphere: 'vSphere',
	pbs: 'PBS',
	pdm: 'PDM',
	pmg: 'PMG',
	unifi: 'UniFi',
	omada: 'Omada',
	plex: 'Plex',
	loki: 'Loki',
	aws: 'AWS',
	gcp: 'GCP',
	apache: 'Apache',
	frigate: 'Frigate',
	nut: 'NUT',
	synology: 'Synology',
	sophos: 'Sophos',
	entra: 'Entra ID'
};

/** Services an agent found (`profile_id` = its module). */
const MODULE_NAME: Record<string, string> = {
	docker: 'Docker',
	plakar: 'Plakar',
	backup_repos: 'restic/Borg',
	zfs: 'ZFS',
	libvirt: 'libvirt',
	wireguard: 'WireGuard',
	ipsec: 'IPsec'
};

/** Kinds whose tag is a word, so translated. */
const WORD: Record<string, () => string> = {
	agent: () => m.devices_kindtag_agent(),
	push: () => m.devices_kindtag_push(),
	domain: () => m.devices_kindtag_domain(),
	ip_blacklist: () => m.devices_kindtag_blocklist(),
	mail_blacklist: () => m.devices_kindtag_reputation(),
	netconfig: () => m.devices_kindtag_netconfig(),
	website: () => m.devices_kindtag_website(),
	webchange: () => m.devices_kindtag_webchange(),
	dummy: () => m.devices_kindtag_demo()
};

const MODULE_WORD: Record<string, () => string> = {
	databases: () => m.devices_kindtag_databases(),
	speedtest: () => m.devices_kindtag_speedtest()
};

export function kindTag(target: Pick<Target, 'kind' | 'profile_id'>, label: string): string {
	if (target.kind === 'agent_service') {
		const mod = target.profile_id ?? '';
		return MODULE_NAME[mod] ?? MODULE_WORD[mod]?.() ?? m.devices_kindtag_service();
	}
	return NAME[target.kind] ?? WORD[target.kind]?.() ?? label;
}
