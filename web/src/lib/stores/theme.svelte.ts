/**
 * Light / dark theme.
 *
 * The app is one console (the `obsidian` class on `<html>`: Inter, the
 * console's tokens). It is deep black by default; Settings › Appearance can
 * turn it light (`console-light` alongside `obsidian`) or make it follow the
 * system. The wall, a room monitor, stays dark. That choice is kept per
 * browser (`theme-choice.ts`).
 *
 * The public status pages (`/s/…`, or a status page's own domain) wear the same
 * console tokens; their light follows the visitor's system
 * (`prefers-color-scheme`) unless the page fixes it. The effective theme is
 * applied to `<html>` as a `dark` class, as Tailwind expects.
 */
import { browser } from '$app/env';
import { page } from '$app/state';
import { routePath } from '#lib/paths.js';
import { hostedStatusSlug } from '#lib/status-host.js';
import { CONSOLE_THEME_KEY, parseConsoleTheme, resolveConsoleTheme, type ConsoleTheme } from './theme-choice.js';

export type { ConsoleTheme } from './theme-choice.js';

/** Class of the console tokens on `<html>` (app.css). */
export const CONSOLE_CLASS = 'obsidian';
/** Added next to it for the light console. */
export const CONSOLE_LIGHT_CLASS = 'console-light';

const BAR_COLOR = { dark: '#07080a', light: '#f6f6f7' } as const;

function readConsoleChoice(): ConsoleTheme {
	if (!browser) return 'dark';
	try {
		return parseConsoleTheme(localStorage.getItem(CONSOLE_THEME_KEY));
	} catch {
		return 'dark';
	}
}

function systemIsDark(): boolean {
	return browser && window.matchMedia('(prefers-color-scheme: dark)').matches;
}

class Theme {
	/** The console's choice, from Settings › Appearance. */
	consoleChoice = $state<ConsoleTheme>('dark');
	/** True when the system is dark; tracked live for automatic mode. */
	#systemDark = $state(false);

	constructor() {
		this.consoleChoice = readConsoleChoice();
		this.#systemDark = systemIsDark();
	}

	/** False on a public status page (the system decides its light); the console everywhere else. */
	get console(): boolean {
		if (!browser) return true;
		return !hostedStatusSlug && !routePath(page.url).startsWith('/s/');
	}

	/** Theme actually applied, once the preference is resolved. */
	get resolved(): 'light' | 'dark' {
		if (this.console) {
			return resolveConsoleTheme(this.consoleChoice, this.#systemDark, browser ? routePath(page.url) : '/');
		}
		return this.#systemDark ? 'dark' : 'light';
	}

	/**
	 * Listens for system theme changes. Call once from the root layout; returns
	 * the unsubscribe function.
	 */
	watchSystem(): () => void {
		if (!browser) return () => {};
		const query = window.matchMedia('(prefers-color-scheme: dark)');
		const onChange = (event: MediaQueryListEvent) => {
			this.#systemDark = event.matches;
		};
		query.addEventListener('change', onChange);
		return () => query.removeEventListener('change', onChange);
	}

	/**
	 * Applies the theme to the document. Called by an effect in the root layout;
	 * a status page passes the light its own setting fixed.
	 */
	apply(fixed?: 'light' | 'dark') {
		if (!browser) return;
		const root = document.documentElement;
		const resolved = fixed ?? this.resolved;
		root.classList.add(CONSOLE_CLASS);
		root.classList.toggle(CONSOLE_LIGHT_CLASS, resolved === 'light');
		root.classList.toggle('dark', resolved === 'dark');
		document
			.querySelector('meta[name="apple-mobile-web-app-status-bar-style"]')
			?.setAttribute('content', resolved === 'dark' ? 'black-translucent' : 'default');
		document.querySelector('meta[name="theme-color"]')?.setAttribute('content', BAR_COLOR[resolved]);
	}

	/** The console's theme (Settings › Appearance), remembered in this browser. */
	setConsole(choice: ConsoleTheme) {
		this.consoleChoice = choice;
		if (!browser) return;
		try {
			if (choice === 'dark') localStorage.removeItem(CONSOLE_THEME_KEY);
			else localStorage.setItem(CONSOLE_THEME_KEY, choice);
		} catch {
			// Not remembered: the console is dark again on the next visit.
		}
	}
}

export const theme = new Theme();
