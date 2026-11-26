/**
 * Where a wall-display account (`wall` role) belongs: `/wall`, and nowhere
 * else. The root layout asks this on every navigation and does the redirect
 * itself (routing lives in `src/routes/+layout.svelte` only); the server
 * refuses everything but the wall's own reads anyway.
 */

/** `/wall`, with or without its own options (`?city=paris`). */
const WALL_PATH = /^\/wall(?:[?#][^\\\u0000-\u001f\u007f]*)?$/;

/** True when `value` is the wall page, options included, and nothing that could leave it. */
export function isWallPath(value: string): boolean {
	return WALL_PATH.test(value);
}

/**
 * Where to send a wall-display account that is on `path`: `null` when it is
 * already on the wall, else the address to replace the current one with. A
 * `redirect` remembered by the sign-in screen is kept when it is the wall
 * itself (so a display's `?city=` survives signing in again);
 * any other destination is ignored.
 */
export function wallDestination(path: string, redirect: string | null = null): string | null {
	if (path === '/wall') return null;
	if (redirect && isWallPath(redirect)) return redirect;
	return '/wall';
}
