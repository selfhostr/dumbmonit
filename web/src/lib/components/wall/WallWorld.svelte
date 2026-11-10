<script lang="ts">
	/**
	 * The wall's world, loaded on demand: Paris behind the board, under a sky
	 * that follows the real hour, with the weather of the network (the rooftop,
	 * the Eiffel Tower, the pigeons: Gaston with his sign, Mimi and her
	 * croissant, Jules on the lamp post, a small moment every 40 to 80 s), drawn
	 * by day and by night (the wall itself is always Night). The scene is its own chunk, fetched when it is first
	 * shown, so the rest of the app never carries any of it.
	 *
	 * Decorative: `aria-hidden`, the board says everything in words.
	 */
	import type { Component } from 'svelte';
	import type { WallCity } from '#lib/components/wall/wallScene.js';
	import { sceneMoodFor, type WallWeather } from '#lib/components/wall/wallWeather.js';
	import type { ScenePulse } from '#lib/scene/mood.js';
	import WallStorm from '#lib/components/wall/WallStorm.svelte';

	interface Props {
		city: WallCity;
		now: Date;
		weather: WallWeather;
		/** Problems on the board: the number on Gaston's sign. */
		problems: number;
		/** Bumped when a new problem appears: the pigeons startle once. */
		startle?: number;
		/** The tab is hidden, or the screen is a phone: nothing moves. */
		paused?: boolean;
		pulse?: ScenePulse | null;
		/** Stage x kept in view on a narrow screen. */
		focus?: number;
	}

	let { city, now, weather, problems, startle = 0, paused = false, pulse = null, focus = 1360 }: Props = $props();

	const LOADERS: Record<WallCity, () => Promise<{ default: Component<any> }>> = {
		paris: () => import('#lib/components/wall/paris/ParisScene.svelte')
	};

	// The scene on screen: set once its chunk has arrived, replaced when the city changes.
	let loaded = $state.raw<{ city: WallCity; scene: Component<any> } | null>(null);
	$effect(() => {
		const wanted = city;
		let alive = true;
		LOADERS[wanted]()
			.then((mod) => {
				if (alive) loaded = { city: wanted, scene: mod.default };
			})
			.catch(() => {
				// A chunk that cannot be fetched (offline): the board stands on its own.
			});
		return () => {
			alive = false;
		};
	});

	const Scene = $derived(loaded && loaded.city === city ? loaded.scene : null);
</script>

<div class="world" aria-hidden="true">
	{#if Scene}
		{#key city}
			<div class="fade-in">
				<Scene theme="dark" {now} mood={sceneMoodFor(weather)} {problems} {startle} {paused} {pulse} {focus} />
				{#if weather === 'thunder'}
					<WallStorm theme="dark" {paused} />
				{/if}
			</div>
		{/key}
	{/if}
</div>

<style>
	.world,
	.fade-in {
		position: absolute;
		inset: 0;
	}
	/* Its own stacking context: nothing a scene draws climbs above the board. */
	.world {
		isolation: isolate;
		overflow: hidden;
		pointer-events: none;
	}
	.fade-in {
		animation: world-in 900ms var(--ease-out-expo, ease) both;
	}
	@keyframes world-in {
		from {
			opacity: 0;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.fade-in {
			animation: none;
		}
	}
</style>
