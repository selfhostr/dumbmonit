/**
 * API surface exposed to components.
 *
 * Each function maps to a server route. Those marked "anticipated" target
 * endpoints that do not exist yet: they degrade gracefully (empty array,
 * `null`) instead of failing the whole page.
 */
export * from './types';
export * from './oncall';
export { onMutation } from './mutations';
export * from './teams';
export * from './clients';
export * from './mail_server';
export * from './channel_templates';
export * from './features';
export * from './slos';
export * from './annotations';
export * from './alert_insights';
export {
	ApiError,
	toApiError,
	setUnauthorizedHandler,
	setTotpSetupHandler,
	TOTP_SETUP_REQUIRED,
	setDemoRefusalHandler,
	setClientView,
	getClientView
} from './client';
export * from './core';
export * from './alerts_api';
export * from './discover';
export * from './auth_api';
export * from './notify_api';
export * from './agents_api';
export * from './status_pages';
export * from './onboarding';
export * from './features';
export * from './overview';
