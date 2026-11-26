<script lang="ts" module>
	export interface BandFigure {
		id: string;
		label: string;
		value: string;
		unit: string;
		note: string;
		/** Text colour class of the note (the figure itself stays in ink). */
		tone: string;
		/** Small graph under the figure (null = gap). Tiles without one keep the row empty. */
		series?: (number | null)[];
		seriesMin?: number;
		seriesMax?: number;
	}
</script>

<script lang="ts">
	/**
	 * The strip along the foot of the wall: the fleet figures. Quiet on
	 * purpose (the picture and the verdict come first): it sits on the stone
	 * face of the rooftop, under the pigeons, on a veil that keeps its figures
	 * readable. On a phone it is the last block of the page.
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
				<span class="graph">
					{#if figure.series}
						<Spark values={figure.series} height={28} min={figure.seriesMin} max={figure.seriesMax} color="var(--color-info)" />
					{/if}
				</span>
				<span class="note {figure.tone}">{figure.note}</span>
			</li>
		{/each}
	</ul>
</div>

<style>
	.band {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		min-width: 0;
		padding: 1rem var(--band-pad, 1rem);
		/* A veil, not a panel: the stone shows through and the figures stay readable. */
		background: color-mix(in srgb, var(--c-canvas) 80%, transparent);
		border-top: 1px solid var(--c-line);
	}
	.figures {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		align-items: start;
		gap: 0.75rem 1rem;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.figure {
		display: grid;
		grid-template-rows: auto auto 1.75rem auto;
		gap: 0.125rem;
		min-width: 0;
		align-content: start;
	}
	.graph {
		display: block;
		min-width: 0;
		height: 1.75rem;
	}
	.label {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 0.8125rem;
		color: var(--c-ink-2);
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
	.note {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 0.75rem;
	}
	/*
	 * From 1024 px the strip lies along the foot of the screen, on the stone face
	 * of the rooftop (about a tenth of the height): the figures, left.
	 */
	@media (min-width: 1024px) {
		.band {
			padding: 0.625rem var(--band-pad, 3.5rem);
			border-top: 0;
			background: linear-gradient(to bottom, transparent, color-mix(in srgb, var(--c-canvas) 78%, transparent) 38%);
		}
		.figures {
			gap: 1.5rem;
		}
		.value {
			font-size: 2.25rem;
		}
	}
	/* A short screen (720p to 900p): the figures lose their notes to stay on the stone. */
	@media (min-width: 1024px) and (max-height: 940px) {
		.note {
			display: none;
		}
		.figure {
			grid-template-rows: auto auto 1.75rem;
		}
		.value {
			font-size: 1.75rem;
		}
		.band {
			padding-block: 0.375rem;
		}
	}
</style>
