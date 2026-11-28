/**
 * What a wall display remembers beyond its scene: the clock's format, a sound
 * for a new critical problem, and the rotation of the monitor pages. Kept per
 * browser, like the scene. Also the address parameters that configure one
 * display without touching what is remembered (`?theme=`, `?scene=`).
 *
 * Pure module (no Svelte): also loaded by `tests/wall-prefs.test.mjs`.
 */
import { toWallSceneChoice, type WallSceneChoice } from './wallScene.ts';

export type ClockFormat = '24h' | '12h';

export interface WallPrefs {
	clock: ClockFormat;
	/** Seconds on the clock. */
	seconds: boolean;
	/** A short beep when a new critical problem appears. */
	criticalSound: boolean;
	/** Seconds between two pages of monitors when they overflow; 0 = no rotation. */
	rotateSeconds: number;
}

export const DEFAULT_WALL_PREFS: WallPrefs = { clock: '24h', seconds: false, criticalSound: false, rotateSeconds: 0 };

/** The delays Settings offers; 0 is off. */
export const ROTATE_CHOICES: readonly number[] = [0, 15, 30, 60];

const STORAGE_KEY = 'dumbmonit-wall-prefs';

/** A stored value as preferences; anything unknown falls back to the default. */
export function parseWallPrefs(raw: string | null): WallPrefs {
	if (!raw) return { ...DEFAULT_WALL_PREFS };
	try {
		const data = JSON.parse(raw) as Partial<WallPrefs> | null;
		const rotate = Number(data?.rotateSeconds);
		return {
			clock: data?.clock === '12h' ? '12h' : '24h',
			seconds: data?.seconds === true,
			criticalSound: data?.criticalSound === true,
			rotateSeconds: ROTATE_CHOICES.includes(rotate) ? rotate : 0
		};
	} catch {
		return { ...DEFAULT_WALL_PREFS };
	}
}

export function readWallPrefs(): WallPrefs {
	try {
		return parseWallPrefs(localStorage.getItem(STORAGE_KEY));
	} catch {
		return { ...DEFAULT_WALL_PREFS };
	}
}

export function writeWallPrefs(prefs: WallPrefs): void {
	try {
		localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
	} catch {
		// Not remembered: it applies until the page reloads.
	}
}

/** The clock's text: 24 h without seconds unless the preferences say otherwise. */
export function clockText(date: Date, locale: string, prefs: Pick<WallPrefs, 'clock' | 'seconds'>): string {
	const twelve = prefs.clock === '12h';
	return date.toLocaleTimeString(locale, {
		hour: twelve ? 'numeric' : '2-digit',
		minute: '2-digit',
		second: prefs.seconds ? '2-digit' : undefined,
		hour12: twelve
	});
}

export interface WallParams {
	theme: 'dark' | 'light' | null;
	scene: WallSceneChoice | null;
}

/** The address parameters of one display; unknown values are ignored. */
export function parseWallParams(params: Pick<URLSearchParams, 'get'>): WallParams {
	const theme = params.get('theme');
	return {
		theme: theme === 'dark' || theme === 'light' ? theme : null,
		scene: toWallSceneChoice(params.get('scene')?.trim().toLowerCase() ?? null)
	};
}

/** Page to show after `page` when the monitors overflow onto `pages` pages (wraps to the first). */
export function nextPage(page: number, pages: number): number {
	return pages <= 1 ? 0 : (page + 1) % pages;
}
