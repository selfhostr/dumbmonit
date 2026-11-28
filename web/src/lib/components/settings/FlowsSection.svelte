<script lang="ts">
	/**
	 * Settings → System → Network flows (beta, administrators): turn the
	 * NetFlow / IPFIX and sFlow receivers on, choose their UDP ports, and see
	 * whether each one listens, what came in since the server started, which
	 * devices export flows and which unknown senders were dropped. Only flows
	 * from the address of a monitored device are kept.
	 */
	import { m } from '#lib/paraglide/messages.js';
	import { withBase } from '#lib/paths.js';
	import { getFlowSettings, setFlowSettings } from '#lib/api/flows.js';
	import type { FlowListener, FlowOverview } from '#lib/api/types.js';
	import { formatGroupedCount, formatRelative } from '#lib/format.js';
	import { Button, DocLink, ErrorNotice, Field, Panel, Plate, Skeleton, Toggle } from '#lib/ui/index.js';

	let data = $state<FlowOverview | null>(null);
	let loading = $state(true);
	let error = $state<unknown>(null);

	let enabled = $state(false);
	let netflowPort = $state(2055);
	let sflowPort = $state(6343);
	let netflowError = $state<string | null>(null);
	let sflowError = $state<string | null>(null);
	let saving = $state(false);
	let saved = $state(false);
	let saveError = $state<unknown>(null);

	function fill(value: FlowOverview) {
		data = value;
		enabled = value.settings.enabled;
		netflowPort = value.settings.netflow_port;
		sflowPort = value.settings.sflow_port;
	}

	async function load(signal?: AbortSignal) {
		loading = true;
		error = null;
		try {
			fill(await getFlowSettings(signal));
		} catch (cause) {
			if (cause instanceof DOMException && cause.name === 'AbortError') return;
			error = cause;
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		const controller = new AbortController();
		void load(controller.signal);
		return () => controller.abort();
	});

	function validPort(value: number): boolean {
		return Number.isInteger(value) && value >= 1 && value <= 65535;
	}

	function validate(): boolean {
		netflowError = validPort(Number(netflowPort)) ? null : m.flows_settings_port_invalid();
		sflowError = validPort(Number(sflowPort)) ? null : m.flows_settings_port_invalid();
		if (!netflowError && !sflowError && Number(netflowPort) === Number(sflowPort)) {
			sflowError = m.flows_settings_ports_clash();
		}
		return !netflowError && !sflowError;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		saved = false;
		saveError = null;
		if (!validate()) return;
		saving = true;
		try {
			fill(await setFlowSettings({ enabled, netflow_port: Number(netflowPort), sflow_port: Number(sflowPort) }));
			saved = true;
			// The receivers reopen their ports in the background: read their state again.
			setTimeout(() => void load(), 2000);
		} catch (cause) {
			saveError = cause;
		} finally {
			saving = false;
		}
	}

	function protocolLabel(listener: FlowListener): string {
		return listener.protocol === 'netflow' ? m.flows_settings_protocol_netflow() : listener.protocol === 'sflow' ? m.flows_settings_protocol_sflow() : listener.protocol;
	}

	function listenerLabel(listener: FlowListener): string {
		if (listener.listening) return m.flows_settings_listening({ port: listener.port });
		return listener.error ? m.flows_settings_not_listening() : m.flows_settings_off();
	}
</script>

<Panel id="flows" title={m.flows_settings_title()} description={m.flows_settings_description()}>
	{#snippet aside()}
		<Plate tone="advisory" label={m.flows_beta()} />
	{/snippet}

	{#if error}
		<ErrorNotice {error} title={m.flows_settings_read_error()} onretry={() => void load()} />
	{:else if loading && !data}
		<Skeleton class="h-32 w-full" />
	{:else if data}
		<div class="grid gap-8">
			<form class="grid gap-5" onsubmit={save} novalidate>
				<p class="max-w-prose text-sm text-ink">{m.flows_settings_filter_note()}</p>
				<Field label={m.flows_settings_enabled()} for="flows-enabled" inline>
					<Toggle id="flows-enabled" bind:checked={enabled} disabled={saving} label={m.flows_settings_enabled()} />
				</Field>
				<div class="grid gap-5 sm:grid-cols-2">
					<Field label={m.flows_settings_netflow_port()} for="flows-netflow-port" error={netflowError} help={m.flows_settings_port_help()}>
						<input
							id="flows-netflow-port"
							type="number"
							min="1"
							max="65535"
							step="1"
							class="input max-w-40"
							bind:value={netflowPort}
							disabled={saving}
							aria-invalid={netflowError ? 'true' : undefined}
							oninput={() => (netflowError = null)}
						/>
					</Field>
					<Field label={m.flows_settings_sflow_port()} for="flows-sflow-port" error={sflowError} help={m.flows_settings_port_help()}>
						<input
							id="flows-sflow-port"
							type="number"
							min="1"
							max="65535"
							step="1"
							class="input max-w-40"
							bind:value={sflowPort}
							disabled={saving}
							aria-invalid={sflowError ? 'true' : undefined}
							oninput={() => (sflowError = null)}
						/>
					</Field>
				</div>
				{#if saveError}
					<ErrorNotice error={saveError} title={m.flows_settings_save_error()} />
				{/if}
				<div class="flex flex-wrap items-center gap-3" aria-live="polite">
					<Button type="submit" variant="primary" loading={saving} success={saved}>{m.flows_settings_save()}</Button>
					{#if saved}<Plate tone="signal" draw label={m.flows_settings_saved()} />{/if}
					<DocLink page="devices/flows/" />
				</div>
			</form>

			<section class="grid gap-3" aria-labelledby="flows-state">
				<h3 id="flows-state" class="text-sm font-semibold text-ink">{m.flows_settings_state_title()}</h3>
				<ul class="grid gap-2">
					{#each data.listeners as listener (listener.protocol)}
						<li class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
							<span class="min-w-0 font-medium text-ink">{protocolLabel(listener)}</span>
							<Plate tone={listener.listening ? 'signal' : listener.error ? 'warning' : 'ghost'} label={listenerLabel(listener)} />
							{#if listener.error}<span class="min-w-0 basis-full break-words text-[0.8125rem] text-ink-2">{listener.error}</span>{/if}
						</li>
					{/each}
				</ul>
				<dl class="grid grid-cols-[1fr_auto] gap-x-6 gap-y-1 text-sm sm:max-w-md">
					<dt class="text-ink-2">{m.flows_settings_received()}</dt>
					<dd class="text-right tabular-nums text-ink">{formatGroupedCount(data.counters.datagrams)}</dd>
					<dt class="text-ink-2">{m.flows_settings_flows()}</dt>
					<dd class="text-right tabular-nums text-ink">{formatGroupedCount(data.counters.flows)}</dd>
					<dt class="text-ink-2">{m.flows_settings_dropped_unknown()}</dt>
					<dd class="text-right tabular-nums text-ink">{formatGroupedCount(data.counters.dropped_unknown)}</dd>
					<dt class="text-ink-2">{m.flows_settings_dropped_malformed()}</dt>
					<dd class="text-right tabular-nums text-ink">{formatGroupedCount(data.counters.dropped_malformed)}</dd>
					{#if data.counters.missing_template > 0}
						<dt class="text-ink-2">{m.flows_settings_missing_template()}</dt>
						<dd class="text-right tabular-nums text-ink">{formatGroupedCount(data.counters.missing_template)}</dd>
					{/if}
					{#if data.counters.series_capped > 0}
						<dt class="text-ink-2">{m.flows_settings_series_capped()}</dt>
						<dd class="text-right tabular-nums text-ink">{formatGroupedCount(data.counters.series_capped)}</dd>
					{/if}
				</dl>
				<p class="text-[0.8125rem] text-ink-3">{m.flows_settings_since_start()}</p>
			</section>

			<section class="grid gap-2" aria-labelledby="flows-exporters">
				<h3 id="flows-exporters" class="text-sm font-semibold text-ink">{m.flows_settings_exporters()}</h3>
				{#if data.exporters.length === 0}
					<p class="text-sm text-ink-2">{m.flows_settings_exporters_none()}</p>
				{:else}
					<ul class="grid divide-y divide-line">
						{#each data.exporters as exporter (`${exporter.target_id}-${exporter.protocol}`)}
							<li class="flex min-h-10 flex-wrap items-center gap-x-3 gap-y-0.5 py-1.5 text-sm">
								<a class="inline-flex min-h-10 min-w-0 items-center font-medium text-ink hover:underline" href={withBase(`/targets/${exporter.target_id}`)}>{exporter.target_name}</a>
								<span class="font-mono text-ink-2">{exporter.address}</span>
								<span class="text-ink-2">{exporter.protocol === 'sflow' ? m.flows_settings_protocol_sflow() : m.flows_settings_protocol_netflow()}</span>
								<span class="ml-auto text-ink-3">{m.flows_settings_exporter_seen({ count: formatGroupedCount(exporter.datagrams), when: formatRelative(exporter.last_seen) })}</span>
							</li>
						{/each}
					</ul>
				{/if}
			</section>

			{#if data.unknown_sources.length > 0}
				<section class="grid gap-2" aria-labelledby="flows-unknown">
					<h3 id="flows-unknown" class="text-sm font-semibold text-ink">{m.flows_settings_unknown_title()}</h3>
					<p class="max-w-prose text-[0.8125rem] text-ink-2">{m.flows_settings_unknown_help()}</p>
					<ul class="grid gap-1">
						{#each data.unknown_sources as source (source.address)}
							<li class="flex flex-wrap items-center gap-x-3 text-sm">
								<span class="font-mono text-ink">{source.address}</span>
								<span class="text-ink-3">{formatRelative(source.last_seen)}</span>
							</li>
						{/each}
					</ul>
				</section>
			{/if}
		</div>
	{/if}
</Panel>
