<script lang="ts" module>
	export interface BandFigure {
		id: string;
		label: string;
		value: string;
		unit: string;
		/** Small graph beside the figure (null = gap). */
		series?: (number | null)[];
		seriesMin?: number;
		seriesMax?: number;
	}
</script>

<script lang="ts">
	/**
	 * The foot of the wall: two fleet figures, monitors up and availability.
	 * From 1024 px they sit under the problems, in the left column, low on the
	 * stone of the quay; the rest of the foot belongs to the scene. On a phone
	 * it is the last block of the page.
	 *
	 * Same numbers as the console (`fleetStore`); the wall computes them.
	 */
	import { m } from '#lib/paraglide/messages.js';
	import Spark from '#lib/components/overview/dash/Spark.svelte';

	interface Props {
		figures: BandFigure[];
	}

	let { figures }: Props = $props();
</script>

<div class="band" data-wall-info>
	<ul class="figures" aria-label={m.misc_nav_overview()}>
		{#each figures as figure (figure.id)}
			<li class="figure">
				<span class="label">{figure.label}</span>
				<span class="value tnum">{figure.value}{#if figure.unit}<span class="unit">{figure.unit}</span>{/if}</span>
				{#if figure.series}
					<span class="graph">
						<Spark values={figure.series} height={24} min={figure.seriesMin} max={figure.seriesMax} color="var(--color-info)" />
					</span>
				{/if}
			</li>
		{/each}
	</ul>
</div>

<style>
	.band {
		min-width: 0;
		padding: 1rem var(--band-pad, 1rem);
		/* A veil, not a panel: the stone shows through and the figures stay readable. */
		background: color-mix(in srgb, var(--c-canvas) 80%, transparent);
		border-top: 1px solid var(--c-line);
	}
	.figures {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1.6fr);
		align-items: start;
		gap: 0.75rem 1.5rem;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.figure {
		display: grid;
		gap: 0.125rem;
		min-width: 0;
		align-content: start;
	}
	.graph {
		display: block;
		min-width: 0;
		height: 1.5rem;
		margin-top: 0.25rem;
	}
	.label {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 1rem;
		font-weight: 600;
		color: var(--c-ink);
	}
	.value {
		font-family: var(--font-code, ui-monospace, monospace);
		font-size: 2rem;
		font-weight: 500;
		letter-spacing: -0.04em;
		line-height: 1.1;
		color: var(--c-ink);
	}
	.unit {
		margin-left: 0.375rem;
		font-family: var(--font-ui, ui-sans-serif, system-ui, sans-serif);
		font-size: 0.9375rem;
		font-weight: 400;
		letter-spacing: 0;
		color: var(--c-ink-2);
	}
	/*
	 * From 1024 px: no veil, the figures sit low on the quay under the problems,
	 * a soft halo keeps them readable over the stone like the headline.
	 */
	@media (min-width: 1024px) {
		.band {
			padding: 0 0 1rem var(--band-pad, 3.5rem);
			border-top: 0;
			background: none;
		}
		.label,
		.value {
			text-shadow: 0 0 14px color-mix(in srgb, var(--c-canvas) 90%, transparent);
		}
		.value {
			font-size: 2.25rem;
		}
	}
	/* A short screen (720p to 900p): smaller figures, closer to the edge. */
	@media (min-width: 1024px) and (max-height: 940px) {
		.value {
			font-size: 1.75rem;
		}
		.band {
			padding-bottom: 0.625rem;
		}
	}
</style>
