<script lang="ts">
	/**
	 * One public service on one tight row: its name, its state (icon and word),
	 * the availability over the bar's period, then the daily history bar. The
	 * bar's tooltip carries the detail of each day.
	 */
	import { m } from '#lib/paraglide/messages.js';
	import type { PublicStatusItem } from '#lib/api/index.js';
	import { formatPercent } from '#lib/format.js';
	import { Plate } from '#lib/ui/index.js';
	import UptimeBar from './UptimeBar.svelte';
	import LatencyGraph from './LatencyGraph.svelte';
	import { ITEM_STATE } from './words';

	interface Props {
		item: PublicStatusItem;
		days: number;
		/** What the bars cover (seconds) and how long one bar is; see `PublicStatus.page`. */
		windowSecs?: number;
		stepSecs?: number;
		/** Name of the period for the availability readout ("Last 12 hours"). */
		periodLabel?: string;
		onselectday?: (day: string) => void;
	}

	let { item, days, windowSecs = 0, stepSecs = 86400, periodLabel = '', onselectday }: Props = $props();

	const state = $derived(ITEM_STATE[item.state] ?? ITEM_STATE.unknown);
	const over = $derived(stepSecs < 86400 && periodLabel ? periodLabel : m.status_service_row_uptime_over({ days }));
</script>

<li class="px-3 py-2.5 sm:px-4">
	<div class="flex items-center gap-2">
		<span class="min-w-0 truncate font-semibold text-ink">{item.label}</span>
		{#if item.external}
			<span class="shrink-0 rounded-full border border-line px-1.5 text-[0.6875rem] font-semibold text-ink-2" title={m.status_service_external_hint()}>{m.status_service_external()}</span>
		{/if}
		<span class="ml-auto flex shrink-0 items-center gap-2.5">
			<Plate tone={state.tone} label={state.label} pulse={item.state === 'down'} />
			<span class="tnum min-w-[3.25rem] text-right text-sm font-semibold text-ink" title={over}>{formatPercent(item.uptime_history)}</span>
		</span>
	</div>
	<div class="mt-2">
		<UptimeBar history={item.history} label={item.label} {stepSecs} {windowSecs} onselect={onselectday} />
	</div>
	<LatencyGraph series={item.latency_series} label={item.label} />
</li>
