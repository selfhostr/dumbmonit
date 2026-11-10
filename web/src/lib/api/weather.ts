/**
 * The wall's weather: the place and unit chosen in Settings, and the
 * temperature outside. The server asks Open-Meteo, never the browser. Mirrors
 * `crates/server/src/api/wall_weather.rs`.
 */
import { request } from './client';
import type { WallWeather, WallWeatherPlace } from './types';

export function getWallWeather(signal?: AbortSignal): Promise<WallWeather> {
	return request<WallWeather>('/wall/weather', { signal, anticipated: true });
}

/** Chooses the place and the unit (admin); `place: null` turns the weather off. */
export function setWallWeather(body: { place: WallWeatherPlace | null; unit: 'c' | 'f' }): Promise<WallWeather> {
	return request<WallWeather>('/wall/weather', { method: 'PUT', body });
}

/** Looks a city up by name (admin). */
export function searchWallWeatherPlaces(query: string, lang: string, signal?: AbortSignal): Promise<WallWeatherPlace[]> {
	return request<WallWeatherPlace[]>(`/wall/weather/places?q=${encodeURIComponent(query)}&lang=${encodeURIComponent(lang)}`, { signal });
}
