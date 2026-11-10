<script lang="ts">
	/**
	 * The weather outside, to the right of the date: a small icon and the
	 * temperature, nothing else. The icon is never the only carrier: it has a
	 * written label ("Outside: Rain, 12 °C") for screen readers and as a tooltip.
	 * Shown only when the server has a reading; no place, or a weather service
	 * that does not answer, leaves nothing on the wall (no message).
	 */
	import { Cloud, CloudFog, CloudLightning, CloudMoon, CloudRain, CloudSnow, CloudSun, Moon, Sun } from 'lucide-svelte';
	import type { WallWeather } from '#lib/api/types.js';
	import { m } from '#lib/paraglide/messages.js';
	import { outdoorDegrees, outdoorIcon, type OutdoorIcon } from '#lib/components/wall/outdoor.js';

	interface Props {
		current: NonNullable<WallWeather['current']>;
		unit: WallWeather['unit'];
	}

	let { current, unit }: Props = $props();

	const ICONS = {
		sun: Sun,
		moon: Moon,
		'cloud-sun': CloudSun,
		'cloud-moon': CloudMoon,
		cloud: Cloud,
		'cloud-fog': CloudFog,
		'cloud-rain': CloudRain,
		'cloud-snow': CloudSnow,
		'cloud-lightning': CloudLightning
	} satisfies Record<OutdoorIcon, unknown>;

	const Icon = $derived(ICONS[outdoorIcon(current.condition, current.is_day)]);
	const words = $derived.by(() => {
		switch (current.condition) {
			case 'clear':
				return current.is_day ? m.wall_weather_clear() : m.wall_weather_clear_night();
			case 'partly_cloudy':
				return m.wall_weather_partly_cloudy();
			case 'cloudy':
				return m.wall_weather_cloudy();
			case 'fog':
				return m.wall_weather_fog();
			case 'rain':
				return m.wall_weather_rain();
			case 'snow':
				return m.wall_weather_snow();
			case 'thunder':
				return m.wall_weather_thunder();
		}
	});
	const label = $derived(m.wall_weather_aria({ condition: words, temperature: `${current.temperature} °${unit.toUpperCase()}` }));
</script>

<span class="outdoor tnum" role="img" aria-label={label} title={label} data-wall-info>
	<Icon class="outdoor-icon" aria-hidden="true" strokeWidth={2} />
	<span aria-hidden="true">{outdoorDegrees(current.temperature)}</span>
</span>

<style>
	.outdoor {
		display: inline-flex;
		align-items: center;
		gap: 0.4em;
		font-size: inherit;
		font-weight: 600;
		line-height: 1;
		color: var(--c-ink-2);
		white-space: nowrap;
	}
	.outdoor :global(.outdoor-icon) {
		width: 1.15em;
		height: 1.15em;
		flex-shrink: 0;
	}
</style>
