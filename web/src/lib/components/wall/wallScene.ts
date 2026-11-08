/**
 * The wall's world: the whole screen is Paris under a sky that follows the
 * real hour, with the weather of the network, drawn by day and by night.
 * Paris is the default and `none` is an explicit choice of a plain board.
 * Remembered per browser, set in Settings or from the wall's own "Backdrop"
 * control; a `?city=` on a shared link overrides it for that display without
 * saving. Retired backdrops (New York, Tokyo, Dubai, `auto`, and the older
 * ones before them) fold onto Paris, so an old saved choice or link still
 * lands somewhere.
 *
 * Pure module (no Svelte): also loaded by `tests/wall-scene.test.mjs`.
 */

export type WallCity = 'paris';
export type WallSceneChoice = 'none' | WallCity;

/** Retired backdrops → Paris, the one kept in their place. */
const RETIRED_CITIES: readonly string[] = [
	'auto',
	'newyork',
	'tokyo',
	'dubai',
	'zermatt',
	'matterhorn',
	'london',
	'rome',
	'hongkong',
	'kyoto',
	'rio'
];

const STORAGE_KEY = 'dumbmonit-wall-city';

/** What a display shows until someone chooses otherwise. */
export const DEFAULT_WALL_SCENE: WallSceneChoice = 'paris';

/** A saved or linked value as a choice, or null when it names nothing known. */
export function toWallSceneChoice(value: string | null): WallSceneChoice | null {
	if (value === null) return null;
	if (value === 'none' || value === 'paris') return value;
	return RETIRED_CITIES.includes(value) ? 'paris' : null;
}

/** The saved choice; `fallback` (Paris) when nothing is saved. */
export function readWallScene(fallback: WallSceneChoice = DEFAULT_WALL_SCENE): WallSceneChoice {
	try {
		return toWallSceneChoice(localStorage.getItem(STORAGE_KEY)) ?? fallback;
	} catch {
		return fallback;
	}
}

/** Remembers a choice, `none` included: a plain board is a choice, not the absence of one. */
export function writeWallScene(choice: WallSceneChoice): void {
	try {
		localStorage.setItem(STORAGE_KEY, choice);
	} catch {
		// Not remembered: it applies until the page reloads.
	}
}

/** A `?city=` value, or null when it names no backdrop. */
export function parseForcedWallScene(value: string | null): WallSceneChoice | null {
	return toWallSceneChoice(value);
}

/** The city to draw for a choice, or null for none. */
export function resolveWallScene(choice: WallSceneChoice): WallCity | null {
	return choice === 'none' ? null : 'paris';
}
