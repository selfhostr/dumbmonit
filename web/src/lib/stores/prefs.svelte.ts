/**
 * Display preferences, per browser: 12/24 h clock, first day of the week,
 * density and reduced motion. Density and motion are classes on `<html>`
 * (`density-compact`, `reduce-motion`, see app.css); the time format and the
 * first day are read by `#lib/format.js` and the calendar grid.
 */
import { browser } from '$app/env';
import {
	DEFAULT_PREFS,
	PREFS_KEY,
	firstDayOverride,
	hour12Option,
	parsePrefs,
	type Density,
	type MotionPref,
	type Prefs,
	type TimeFormat,
	type WeekStart
} from './prefs-model.js';

function read(): Prefs {
	if (!browser) return { ...DEFAULT_PREFS };
	try {
		return parsePrefs(localStorage.getItem(PREFS_KEY));
	} catch {
		return { ...DEFAULT_PREFS };
	}
}

class DisplayPrefs {
	timeFormat = $state<TimeFormat>('auto');
	weekStart = $state<WeekStart>('auto');
	density = $state<Density>('comfortable');
	motion = $state<MotionPref>('auto');

	constructor() {
		const stored = read();
		this.timeFormat = stored.timeFormat;
		this.weekStart = stored.weekStart;
		this.density = stored.density;
		this.motion = stored.motion;
	}

	get hour12(): boolean | undefined {
		return hour12Option(this.timeFormat);
	}

	get firstDay(): number | null {
		return firstDayOverride(this.weekStart);
	}

	set(patch: Partial<Prefs>): void {
		Object.assign(this, patch);
		if (!browser) return;
		try {
			const { timeFormat, weekStart, density, motion } = this;
			localStorage.setItem(PREFS_KEY, JSON.stringify({ timeFormat, weekStart, density, motion }));
		} catch {
			// Storage blocked: the choice lasts until the page is closed.
		}
		this.apply();
	}

	/** Puts the classes on `<html>`. Called at start-up and after each change. */
	apply(onWall = location.pathname.replace(/^\/demo(?=\/|$)/, '').startsWith('/wall')): void {
		if (!browser) return;
		const root = document.documentElement;
		// The wall ignores density.
		root.classList.toggle('density-compact', this.density === 'compact' && !onWall);
		root.classList.toggle('reduce-motion', this.motion === 'reduce');
	}
}

export const displayPrefs = new DisplayPrefs();
