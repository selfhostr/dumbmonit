/**
 * Types of the DumbMonit server API contract, split by area under `./types/`.
 * Every area is re-exported here so existing imports keep working.
 */
export * from './types/targets.js';
export * from './types/webchange-netconfig.js';
export * from './types/alerts.js';
export * from './types/collectors.js';
export * from './types/auth.js';
export * from './types/security-https.js';
export * from './types/notify.js';
export * from './types/agents.js';
export * from './types/status-pages.js';
export * from './types/pbs.js';
export * from './types/maps.js';
export * from './types/proxmox.js';
export * from './types/synology.js';
export * from './types/pdm-pmg.js';
export * from './types/settings.js';
export * from './types/opnsense-truenas.js';
export * from './types/redfish-observability.js';
export * from './types/directory-security.js';
export * from './types/updates-websites.js';
export * from './types/discovery.js';
export * from './types/logs-cves.js';
export * from './types/teams-clients.js';
export * from './types/oncall.js';
export * from './types/storage-metrics.js';
export * from './types/remediation-deadlines.js';
export * from './types/overview.js';
export * from './types/flows.js';
