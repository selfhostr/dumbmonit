import { untrack } from 'svelte';
import { getWallWeather } from '#lib/api/weather.js';
import type { WallWeather as OutdoorWeather } from '#lib/api/types.js';
import { ApiError } from '#lib/api/client.js';
import { OUTDOOR_REFRESH_MS, OUTDOOR_RETRY_MS } from '#lib/components/wall/outdoor.js';

/**
 * The weather outside, beside the date. The server asks Open-Meteo (browsers
 * never do) and keeps the reading ten minutes; a failure, or no place chosen
 * in Settings, leaves the wall without it, silently, and the wall asks again
 * after a minute rather than ten, so a city just chosen shows up quickly.
 * Call during component initialisation.
 */
export function useOutdoorWeather(): { readonly value: OutdoorWeather | null } {
	let outdoor = $state<OutdoorWeather | null>(null);
	$effect(() => {
		const controller = new AbortController();
		let timer: ReturnType<typeof setTimeout> | undefined;
		let stopped = false;
		const poll = async () => {
			if (document.visibilityState === 'visible') {
				try {
					outdoor = await getWallWeather(controller.signal);
				} catch (cause) {
					if (cause instanceof DOMException && cause.name === 'AbortError') return;
					// Not shown: keep the last reading, if any, until the next try.
					// A server without the route will not have one: stop asking.
					if (cause instanceof ApiError && cause.missing) stopped = true;
				}
			}
			if (!stopped && !controller.signal.aborted) timer = setTimeout(poll, outdoor?.current ? OUTDOOR_REFRESH_MS : OUTDOOR_RETRY_MS);
		};
		untrack(() => void poll());
		return () => {
			controller.abort();
			clearTimeout(timer);
		};
	});
	return {
		get value() {
			return outdoor;
		}
	};
}

/** Keeps the screen on; browsers release the lock when the tab is hidden. */
export function useWakeLock(): void {
	$effect(() => {
		let lock: WakeLockSentinel | null = null;
		let disposed = false;
		const request = async () => {
			if (disposed || document.visibilityState !== 'visible') return;
			try {
				lock = (await navigator.wakeLock?.request('screen')) ?? null;
			} catch {
				// Not permitted (battery saver, insecure context): the wall still works.
			}
		};
		void request();
		document.addEventListener('visibilitychange', request);
		return () => {
			disposed = true;
			document.removeEventListener('visibilitychange', request);
			void lock?.release().catch(() => {});
		};
	});
}
