<script lang="ts">
	/**
	 * Traffic of a flow exporter (beta) on its device page: one proportional bar
	 * of the largest senders' share, then the top talkers, conversations and
	 * ports over the chosen window. Renders nothing while the device has never
	 * exported flows (most devices), and nothing when the first read fails: the
	 * page does not depend on it. The legend carries the same figures as the bar,
	 * in text, so nothing depends on colour alone.
	 */
	import { getTargetFlows } from '#lib/api/flows.js';
	import type { FlowRange, TargetFlows, TargetId } from '#lib/api/types.js';
	import Segmented from '#lib/components/devices/Segmented.svelte';
	import { formatBinaryBytes, formatGroupedCount } from '#lib/format.js';
	import { RANGES } from '#lib/metrics.js';
	import { m } from '#lib/paraglide/messages.js';
	import { ErrorNotice, Panel, Plate, Skeleton } from '#lib/ui/index.js';

	interface Props {
		target: TargetId;
		class?: string;
	}
	let { target, class: cls = '' }: Props = $props();

	const TOKENS = ['info', 'signal', 'advisory', 'warning'] as const;
	const rangeOptions = RANGES.map((r) => ({ id: r.id as FlowRange, label: r.label }));

	let range = $state<FlowRange>('1h');
	let data = $state<TargetFlows | null>(null);
	/** Becomes true once the device is known to export flows, and stays. */
	let shown = $state(false);
	let loading = $state(false);
	let error = $state<unknown>(null);

	$effect(() => {
		const id = target;
		const span = range;
		const controller = new AbortController();
		loading = true;
		error = null;
		getTargetFlows(id, span, controller.signal).then(
			(flows) => {
				data = flows;
				if (flows.available) shown = true;
				loading = false;
			},
			(cause) => {
				if (cause instanceof DOMException && cause.name === 'AbortError') return;
				error = cause;
				loading = false;
			}
		);
		return () => controller.abort();
	});

	function share(bytes: number): string {
		const total = data?.total_bytes ?? 0;
		return total > 0 ? `${Math.round((bytes / total) * 100)}%` : '—';
	}

	function service(proto: string, port: string): string {
		return port === '0' ? proto : `${port}/${proto}`;
	}

	/** The four largest senders, then everyone else. */
	const parts = $derived.by(() => {
		if (!data || data.total_bytes <= 0) return [];
		const top = data.talkers.slice(0, TOKENS.length).map((t, i) => ({
			key: t.address,
			label: t.address,
			bytes: t.bytes,
			token: TOKENS[i] as string
		}));
		const rest = data.total_bytes - top.reduce((sum, part) => sum + part.bytes, 0);
		if (rest > data.total_bytes * 0.005) {
			top.push({ key: '__others', label: m.flows_traffic_others(), bytes: rest, token: 'ink-3' });
		}
		return top;
	});

	const summary = $derived(parts.map((part) => `${part.label} ${share(part.bytes)}`).join(', '));
</script>

{#if shown}
	<Panel id="device-traffic" title={m.flows_traffic_title()} description={m.flows_traffic_description()} class={cls}>
		{#snippet aside()}
			<Plate tone="advisory" label={m.flows_beta()} />
			<Segmented options={rangeOptions} value={range} onchange={(value) => (range = value)} label={m.flows_traffic_range()} size="sm" />
		{/snippet}

		{#if error}
			<ErrorNotice {error} title={m.flows_traffic_error()} />
		{:else if loading && !data}
			<Skeleton class="h-40 w-full" />
		{:else if data && data.total_bytes <= 0}
			<p class="text-sm text-ink-2">{m.flows_traffic_empty()}</p>
		{:else if data}
			<div class="grid gap-6" aria-busy={loading}>
				<div>
					<p class="text-sm tabular-nums text-ink-2">
						{m.flows_traffic_total({ bytes: formatBinaryBytes(data.total_bytes), packets: formatGroupedCount(data.total_packets) })}
					</p>
					<div
						class="mt-2 flex h-4 w-full gap-px overflow-hidden rounded-[var(--radius-control,0.375rem)] border border-line bg-surface"
						role="img"
						aria-label={m.flows_traffic_bar_label({ summary })}
					>
						{#each parts as part (part.key)}
							<span class="h-full min-w-[2px]" style="flex: {part.bytes} 1 0; background: var(--c-{part.token})"></span>
						{/each}
					</div>
					<ul class="mt-3 grid grid-cols-1 gap-x-6 gap-y-1.5 sm:grid-cols-2 lg:grid-cols-5">
						{#each parts as part (part.key)}
							<li class="flex min-h-6 min-w-0 items-center gap-2 text-sm">
								<span class="size-3 shrink-0 rounded-sm" style="background: var(--c-{part.token})" aria-hidden="true"></span>
								<span class="min-w-0 flex-1 truncate font-mono text-ink-2" title={part.label}>{part.label}</span>
								<span class="w-10 text-right tabular-nums text-ink">{share(part.bytes)}</span>
							</li>
						{/each}
					</ul>
				</div>

				<div class="grid gap-6 lg:grid-cols-2">
					<section aria-labelledby="traffic-talkers">
						<h3 id="traffic-talkers" class="text-sm font-semibold text-ink">{m.flows_traffic_talkers()}</h3>
						<ol class="mt-2 grid divide-y divide-line">
							{#each data.talkers as talker (talker.address)}
								<li class="flex min-h-10 items-center gap-3 py-1.5 text-sm">
									<span class="min-w-0 flex-1 font-mono break-all text-ink">{talker.address}</span>
									<span class="shrink-0 tabular-nums text-ink">{formatBinaryBytes(talker.bytes)}</span>
									<span class="w-10 shrink-0 text-right tabular-nums text-ink-3">{share(talker.bytes)}</span>
								</li>
							{/each}
						</ol>
					</section>

					<section aria-labelledby="traffic-ports">
						<h3 id="traffic-ports" class="text-sm font-semibold text-ink">{m.flows_traffic_ports()}</h3>
						{#if data.ports.length === 0}
							<p class="mt-2 text-sm text-ink-2">{m.flows_traffic_no_ports()}</p>
						{:else}
							<ol class="mt-2 grid divide-y divide-line">
								{#each data.ports as row (`${row.proto}-${row.port}`)}
									<li class="flex min-h-10 items-center gap-3 py-1.5 text-sm">
										<span class="min-w-0 flex-1 font-mono text-ink">{service(row.proto, row.port)}</span>
										<span class="shrink-0 tabular-nums text-ink">{formatBinaryBytes(row.bytes)}</span>
										<span class="w-10 shrink-0 text-right tabular-nums text-ink-3">{share(row.bytes)}</span>
									</li>
								{/each}
							</ol>
						{/if}
					</section>
				</div>

				<section aria-labelledby="traffic-conversations">
					<h3 id="traffic-conversations" class="text-sm font-semibold text-ink">{m.flows_traffic_conversations()}</h3>
					<ol class="mt-2 grid divide-y divide-line">
						{#each data.conversations as row (`${row.src}-${row.dst}-${row.proto}-${row.port}`)}
							<li class="flex min-h-10 flex-wrap items-center gap-x-3 gap-y-0.5 py-1.5 text-sm">
								<span class="min-w-0 flex-1 basis-60 font-mono break-all text-ink">{m.flows_traffic_conversation({ src: row.src, dst: row.dst })}</span>
								<span class="shrink-0 font-mono text-ink-2">{service(row.proto, row.port)}</span>
								<span class="shrink-0 tabular-nums text-ink">{formatBinaryBytes(row.bytes)}</span>
								{#if row.packets != null}
									<span class="shrink-0 tabular-nums text-ink-3">{m.flows_traffic_packets({ count: formatGroupedCount(row.packets) })}</span>
								{/if}
							</li>
						{/each}
					</ol>
				</section>

				<p class="max-w-prose text-[0.8125rem] text-ink-2">{m.flows_traffic_note()}</p>
			</div>
		{/if}
	</Panel>
{/if}
