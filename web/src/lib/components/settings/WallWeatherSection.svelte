<script lang="ts">
	/**
	 * Settings → Wall weather. The outside temperature and a weather icon beside
	 * the wall's date, for every wall. Off until an administrator picks a city
	 * (searched by name on Open-Meteo, by the server: the browser never calls it),
	 * with °C or °F. Each change is saved at once. Anyone may read it; only
	 * administrators change it.
	 */
	import { Check, Search } from 'lucide-svelte';
	import { getWallWeather, searchWallWeatherPlaces, setWallWeather } from '#lib/api/weather.js';
	import type { WallWeather, WallWeatherPlace } from '#lib/api/types.js';
	import { auth } from '#lib/stores/auth.svelte.js';
	import { Button, ErrorNotice, Field, Panel, Skeleton } from '#lib/ui/index.js';
	import { getLocale } from '#lib/paraglide/runtime.js';
	import { m } from '#lib/paraglide/messages.js';

	let setting = $state<WallWeather | null>(null);
	let loading = $state(true);
	let missing = $state(false);
	let error = $state<unknown>(null);
	let saving = $state(false);
	let saveError = $state<unknown>(null);

	let query = $state('');
	let searching = $state(false);
	let results = $state<WallWeatherPlace[] | null>(null);
	let searchError = $state<string | null>(null);

	$effect(() => {
		const controller = new AbortController();
		getWallWeather(controller.signal)
			.then((value) => (setting = value))
			.catch((cause) => {
				if (cause instanceof DOMException && cause.name === 'AbortError') return;
				if (cause && typeof cause === 'object' && 'missing' in cause && cause.missing) missing = true;
				else error = cause;
			})
			.finally(() => (loading = false));
		return () => controller.abort();
	});

	const UNITS = $derived<{ value: 'c' | 'f'; label: string }[]>([
		{ value: 'c', label: m.settings_wall_weather_unit_c() },
		{ value: 'f', label: m.settings_wall_weather_unit_f() }
	]);

	function describe(place: WallWeatherPlace): string {
		return [place.name, place.region, place.country].filter((part): part is string => !!part).join(', ');
	}

	async function save(place: WallWeatherPlace | null, unit: 'c' | 'f') {
		saving = true;
		saveError = null;
		try {
			setting = await setWallWeather({ place, unit });
		} catch (cause) {
			saveError = cause;
		} finally {
			saving = false;
		}
	}

	async function search(event: SubmitEvent) {
		event.preventDefault();
		const text = query.trim();
		if (text.length < 2 || searching) return;
		searching = true;
		searchError = null;
		try {
			results = await searchWallWeatherPlaces(text, getLocale());
		} catch {
			results = null;
			searchError = m.settings_wall_weather_search_error();
		} finally {
			searching = false;
		}
	}

	async function pick(place: WallWeatherPlace) {
		await save(place, setting?.unit ?? 'c');
		if (!saveError) {
			results = null;
			query = '';
		}
	}

	const CHIP = 'min-h-10 rounded-[var(--radius-plate)] border px-3 py-1 text-[0.8125rem] transition-colors';
	const CHIP_ON = 'border-signal bg-signal-soft font-semibold text-ink';
	const CHIP_OFF = 'border-line text-ink-2 hover:bg-surface-2';
</script>

{#if !missing}
	<Panel id="wall-weather" title={m.settings_wall_weather_title()} description={m.settings_wall_weather_description()}>
		{#if error}
			<ErrorNotice {error} title={m.settings_wall_weather_read_error()} />
		{:else if loading || !setting}
			<Skeleton class="h-8 w-full" rows={2} />
		{:else}
			<div class="grid gap-5">
				<p class="m-0 text-sm text-ink" aria-live="polite">
					{setting.place ? m.settings_wall_weather_current({ place: describe(setting.place) }) : m.settings_wall_weather_none()}
				</p>

				{#if auth.isAdmin}
					<form class="flex flex-col gap-2 sm:flex-row sm:items-end" onsubmit={search} novalidate>
						<div class="min-w-0 flex-1">
							<Field label={m.settings_wall_weather_search_label()} for="wall-weather-query" help={m.settings_wall_weather_search_help()} error={searchError}>
								<input id="wall-weather-query" class="input" bind:value={query} maxlength="80" autocomplete="off" disabled={saving} />
							</Field>
						</div>
						<Button type="submit" variant="secondary" loading={searching} disabled={query.trim().length < 2}>
							<Search class="size-4" aria-hidden="true" />
							{m.settings_wall_weather_search_button()}
						</Button>
					</form>

					{#if results}
						{#if results.length === 0}
							<p class="m-0 text-sm text-ink-2">{m.settings_wall_weather_no_results()}</p>
						{:else}
							<ul class="m-0 grid list-none gap-1.5 p-0" aria-label={m.settings_wall_weather_results_label()}>
								{#each results as place (`${place.latitude},${place.longitude}`)}
									<li>
										<button
											type="button"
											class="flex min-h-10 w-full items-center justify-between gap-3 rounded-[var(--radius-plate)] border border-line px-3 py-1.5 text-left text-sm text-ink hover:bg-surface-2 disabled:opacity-60"
											disabled={saving}
											onclick={() => void pick(place)}
										>
											<span class="min-w-0 truncate">{describe(place)}</span>
										</button>
									</li>
								{/each}
							</ul>
						{/if}
					{/if}

					<div class="grid gap-2" role="group" aria-labelledby="wall-weather-unit">
						<p id="wall-weather-unit" class="m-0 text-sm font-semibold text-ink">{m.settings_wall_weather_unit_label()}</p>
						<div class="flex flex-wrap gap-1.5" role="radiogroup" aria-labelledby="wall-weather-unit">
							{#each UNITS as option (option.value)}
								{@const selected = setting.unit === option.value}
								<button
									type="button"
									role="radio"
									aria-checked={selected}
									class={`${CHIP} flex items-center gap-1.5 ${selected ? CHIP_ON : CHIP_OFF}`}
									disabled={saving}
									onclick={() => void save(setting?.place ?? null, option.value)}
								>
									{#if selected}<Check class="size-3.5" aria-hidden="true" />{/if}
									{option.label}
								</button>
							{/each}
						</div>
					</div>

					{#if setting.place}
						<div>
							<Button variant="secondary" loading={saving} onclick={() => void save(null, setting?.unit ?? 'c')}>{m.settings_wall_weather_remove()}</Button>
						</div>
					{/if}
					{#if saveError}<ErrorNotice error={saveError} title={m.settings_wall_weather_save_error()} />{/if}
				{:else}
					<p class="m-0 text-[0.8125rem] text-ink-3">{m.settings_wall_weather_admin_only()}</p>
				{/if}
			</div>
		{/if}
	</Panel>
{/if}
