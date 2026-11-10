/**
 * The outside weather beside the wall's date: which icon says it, how the
 * temperature reads, how often the wall asks. Not to be confused with
 * `wallWeather.ts`, the weather of the scene (rain when monitors are down).
 *
 * Pure module (no Svelte): also loaded by `tests/wall-outdoor.test.mjs`.
 */
import type { WallWeatherCondition } from '#lib/api/types.js';

export type OutdoorIcon = 'sun' | 'moon' | 'cloud-sun' | 'cloud-moon' | 'cloud' | 'cloud-fog' | 'cloud-rain' | 'cloud-snow' | 'cloud-lightning';

/** The server keeps the reading ten minutes: asking more often only returns the same one. */
export const OUTDOOR_REFRESH_MS = 10 * 60_000;

/** One icon per condition; the sun and the partial cloud turn to the moon at night. */
export function outdoorIcon(condition: WallWeatherCondition, isDay: boolean): OutdoorIcon {
	switch (condition) {
		case 'clear':
			return isDay ? 'sun' : 'moon';
		case 'partly_cloudy':
			return isDay ? 'cloud-sun' : 'cloud-moon';
		case 'cloudy':
			return 'cloud';
		case 'fog':
			return 'cloud-fog';
		case 'rain':
			return 'cloud-rain';
		case 'snow':
			return 'cloud-snow';
		case 'thunder':
			return 'cloud-lightning';
	}
}

/** "12°", with the true minus sign below zero and never "-0°". */
export function outdoorDegrees(value: number): string {
	const whole = Math.round(value);
	return `${whole < 0 ? '−' : ''}${Math.abs(whole)}°`;
}
