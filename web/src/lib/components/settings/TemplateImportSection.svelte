<script lang="ts">
	/**
	 * Settings → Integration packs → Import a template. The administrator
	 * picks a Zabbix template export (YAML or XML) or a snmp_exporter
	 * generator.yml / snmp.yml; the server converts it into a pack and answers
	 * with a report (converted, partly converted, ignored, each with its
	 * reason). Nothing is stored until "Install pack": the server then converts
	 * the same file again and installs it like any pack.
	 *
	 * Reasons in the report are English text from the server and are shown as
	 * served.
	 */
	import { FileUp, Plus, RotateCw, ScanSearch } from 'lucide-svelte';
	import {
		applyTemplateImport,
		previewTemplateImport,
		TEMPLATE_IMPORT_MAX_BYTES
	} from '#lib/api/packs.js';
	import type {
		PackView,
		TemplateFormat,
		TemplateImportApplied,
		TemplateImportEntry,
		TemplateImportPreview,
		TemplateImportRequest
	} from '#lib/api/index.js';
	import { Button, Disclosure, ErrorNotice, Field, Plate, Segmented } from '#lib/ui/index.js';
	import { m } from '#lib/paraglide/messages.js';

	interface Props {
		/** Called with the installed pack, so the list above can show it. */
		oninstalled?: (pack: PackView) => void;
	}

	let { oninstalled }: Props = $props();

	let content = $state('');
	let fileName = $state('');
	let fileError = $state<string | null>(null);
	let packId = $state('');
	let label = $state('');
	let sysobjectid = $state('');

	let previewing = $state(false);
	let preview = $state<TemplateImportPreview | null>(null);
	let previewError = $state<unknown>(null);
	let applying = $state(false);
	let applied = $state<TemplateImportApplied | null>(null);
	let applyError = $state<unknown>(null);

	function reset() {
		preview = null;
		previewError = null;
		applied = null;
		applyError = null;
	}

	async function pick(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		fileError = null;
		if (!file) return;
		if (file.size > TEMPLATE_IMPORT_MAX_BYTES) {
			fileError = m.settings_tplimport_file_too_big();
			input.value = '';
			return;
		}
		try {
			content = await file.text();
			fileName = file.name;
			reset();
		} catch {
			fileError = m.settings_tplimport_file_unreadable();
		}
		input.value = '';
	}

	function body(): TemplateImportRequest {
		const prefixes = sysobjectid
			.split(/[\s,]+/)
			.map((part) => part.trim())
			.filter(Boolean);
		return {
			content,
			...(fileName ? { filename: fileName } : {}),
			...(packId.trim() ? { id: packId.trim() } : {}),
			...(label.trim() ? { label: label.trim() } : {}),
			...(prefixes.length > 0 ? { sysobjectid: prefixes } : {})
		};
	}

	async function runPreview(event: SubmitEvent) {
		event.preventDefault();
		if (!content) return;
		previewing = true;
		reset();
		try {
			preview = await previewTemplateImport(body());
			filter = preview.summary.ignored > 0 ? 'ignored' : 'all';
		} catch (cause) {
			previewError = cause;
		} finally {
			previewing = false;
		}
	}

	async function apply() {
		if (!preview?.installable) return;
		applying = true;
		applyError = null;
		try {
			applied = await applyTemplateImport(body());
			oninstalled?.(applied.install.pack);
			preview = { ...preview, exists: true };
		} catch (cause) {
			applyError = cause;
		} finally {
			applying = false;
		}
	}

	const FORMAT: Record<TemplateFormat, () => string> = {
		zabbix_yaml: () => m.settings_tplimport_format_zabbix_yaml(),
		zabbix_xml: () => m.settings_tplimport_format_zabbix_xml(),
		snmp_exporter_generator: () => m.settings_tplimport_format_generator(),
		snmp_exporter_snmp: () => m.settings_tplimport_format_snmp()
	};

	const OUTCOME: Record<TemplateImportEntry['outcome'], { tone: 'signal' | 'advisory' | 'ghost'; label: () => string }> = {
		converted: { tone: 'signal', label: () => m.settings_tplimport_outcome_converted() },
		partial: { tone: 'advisory', label: () => m.settings_tplimport_outcome_partial() },
		ignored: { tone: 'ghost', label: () => m.settings_tplimport_outcome_ignored() }
	};

	/** `all`, or one outcome of the report. */
	let filter = $state<string>('all');
	const filterOptions = $derived(
		preview
			? [
					{ value: 'all', label: m.settings_tplimport_filter_all() },
					{ value: 'converted', label: m.settings_tplimport_filter_converted({ count: preview.summary.converted }) },
					{ value: 'partial', label: m.settings_tplimport_filter_partial({ count: preview.summary.partial }) },
					{ value: 'ignored', label: m.settings_tplimport_filter_ignored({ count: preview.summary.ignored }) }
				]
			: []
	);
	const shown = $derived(
		preview ? preview.report.filter((entry) => filter === 'all' || entry.outcome === filter) : []
	);
	const metrics = $derived(preview ? preview.summary.snmp_metrics + preview.summary.http_metrics : 0);
</script>

<Disclosure title={m.settings_tplimport_title()}>
	<div class="mt-4 grid gap-4">
		<p class="max-w-[68ch] text-sm text-ink-2">{m.settings_tplimport_intro()}</p>
		<p class="max-w-[68ch] text-[0.8125rem] text-ink-2">{m.settings_tplimport_rights()}</p>

		<form class="grid gap-4" onsubmit={runPreview} novalidate>
			<Field label={m.settings_tplimport_file_label()} for="tplimport-file" error={fileError}>
				<label
					class="inline-flex min-h-10 w-fit max-w-full cursor-pointer items-center gap-2 rounded-lg border border-line bg-surface px-3 py-1.5 text-sm font-semibold text-ink transition-colors hover:border-line-strong has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-offset-2 has-[:focus-visible]:outline-signal"
				>
					<FileUp class="size-4 shrink-0" aria-hidden="true" />
					<span class="min-w-0 truncate">
						{fileName ? m.settings_tplimport_loaded({ name: fileName }) : m.settings_tplimport_choose()}
					</span>
					<input
						id="tplimport-file"
						type="file"
						accept=".yaml,.yml,.xml,application/yaml,application/x-yaml,text/yaml,application/xml,text/xml,text/plain"
						class="sr-only"
						onchange={pick}
						disabled={previewing || applying}
					/>
				</label>
			</Field>

			<div class="grid gap-4 sm:grid-cols-2">
				<Field label={m.settings_tplimport_id_label()} for="tplimport-id" help={m.settings_tplimport_id_help()}>
					<input id="tplimport-id" class="input font-mono" autocomplete="off" spellcheck="false" bind:value={packId} oninput={reset} />
				</Field>
				<Field label={m.settings_tplimport_label_label()} for="tplimport-label" help={m.settings_tplimport_label_help()}>
					<input id="tplimport-label" class="input" autocomplete="off" bind:value={label} oninput={reset} />
				</Field>
				<Field label={m.settings_tplimport_sysoid_label()} for="tplimport-sysoid" help={m.settings_tplimport_sysoid_help()} class="sm:col-span-2">
					<input
						id="tplimport-sysoid"
						class="input font-mono"
						autocomplete="off"
						spellcheck="false"
						placeholder="1.3.6.1.4.1.99999"
						bind:value={sysobjectid}
						oninput={reset}
					/>
				</Field>
			</div>

			{#if previewError}
				<ErrorNotice error={previewError} title={m.settings_tplimport_preview_error()} />
			{/if}

			<div>
				<Button type="submit" variant="secondary" loading={previewing} disabled={!content || applying}>
					<ScanSearch class="size-4" aria-hidden="true" />
					{m.settings_tplimport_preview_button()}
				</Button>
			</div>
		</form>

		{#if preview}
			<section class="grid gap-4 rounded-[var(--radius-card)] border border-line bg-surface p-4" aria-labelledby="tplimport-result" aria-live="polite">
				<div class="flex flex-wrap items-center gap-2">
					<h4 id="tplimport-result" class="text-base font-semibold text-ink">{preview.label}</h4>
					<Plate tone="info" label={FORMAT[preview.format]()} />
					{#if preview.exists}<Plate tone="advisory" label={m.settings_tplimport_exists()} />{/if}
				</div>
				<p class="-mt-3 font-mono text-[0.75rem] text-ink-3 break-all">{preview.pack_id}</p>

				<dl class="grid grid-cols-2 gap-3 sm:grid-cols-4">
					<div>
						<dt class="text-[0.8125rem] text-ink-2">{m.settings_tplimport_stat_profiles()}</dt>
						<dd class="tnum text-lg font-semibold text-ink">{preview.summary.snmp_profiles.length}</dd>
					</div>
					<div>
						<dt class="text-[0.8125rem] text-ink-2">{m.settings_tplimport_stat_sources()}</dt>
						<dd class="tnum text-lg font-semibold text-ink">{preview.summary.http_sources}</dd>
					</div>
					<div>
						<dt class="text-[0.8125rem] text-ink-2">{m.settings_tplimport_stat_metrics()}</dt>
						<dd class="tnum text-lg font-semibold text-ink">{metrics}</dd>
					</div>
					<div>
						<dt class="text-[0.8125rem] text-ink-2">{m.settings_tplimport_stat_rules()}</dt>
						<dd class="tnum text-lg font-semibold text-ink">{preview.summary.rules.length}</dd>
					</div>
				</dl>

				{#if preview.errors.length > 0}
					<div role="alert" class="rounded-md border border-warning/35 bg-warning-soft px-4 py-3">
						<p class="font-semibold text-warning-ink">{m.settings_tplimport_errors_title()}</p>
						<ul class="mt-2 grid gap-1 text-sm text-ink">
							{#each preview.errors as item, index (index)}
								<li class="min-w-0 font-mono text-[0.8125rem] break-words">{item}</li>
							{/each}
						</ul>
						<p class="mt-2 text-sm text-ink-2">{m.settings_tplimport_errors_hint()}</p>
					</div>
				{/if}

				<div class="grid gap-3">
					<Segmented
						legend={m.settings_tplimport_report_heading()}
						name="tplimport-filter"
						options={filterOptions}
						bind:value={filter}
					/>
					{#if shown.length === 0}
						<p class="text-sm text-ink-2">{m.settings_tplimport_report_empty()}</p>
					{:else}
						<ul class="grid max-h-[28rem] gap-0 overflow-y-auto rounded-md border border-line">
							{#each shown as entry, index (index)}
								{@const outcome = OUTCOME[entry.outcome]}
								<li class="grid gap-1 border-b border-line px-3 py-2 last:border-b-0 sm:grid-cols-[9rem_minmax(0,1fr)] sm:gap-3">
									<div class="flex flex-wrap items-center gap-2 sm:block">
										<Plate tone={outcome.tone} label={outcome.label()} />
										<span class="text-[0.75rem] text-ink-3 sm:mt-1 sm:block">{entry.element}</span>
									</div>
									<div class="min-w-0">
										<p class="font-mono text-[0.8125rem] break-all text-ink">{entry.name}</p>
										<p class="text-[0.8125rem] text-ink-2">{entry.reason}</p>
									</div>
								</li>
							{/each}
						</ul>
					{/if}
				</div>

				<Disclosure title={m.settings_tplimport_yaml_title()}>
					<pre class="max-h-96 overflow-auto rounded-md bg-canvas-deep p-3 font-mono text-[0.75rem] leading-relaxed text-ink">{preview.yaml}</pre>
				</Disclosure>

				{#if applyError}
					<ErrorNotice error={applyError} title={m.settings_tplimport_apply_error()} />
				{/if}

				<div>
					<Button
						variant="primary"
						loading={applying}
						success={!!applied}
						disabled={!preview.installable || previewing}
						onclick={() => void apply()}
					>
						<Plus class="size-4" aria-hidden="true" />
						{preview.exists ? m.settings_tplimport_update_button() : m.settings_tplimport_apply_button()}
					</Button>
				</div>

				{#if applied}
					<div class="grid gap-3" aria-live="polite">
						<p class="text-sm text-ink">
							{m.settings_tplimport_applied({ label: applied.install.pack.label, id: applied.pack_id })}
						</p>
						{#if applied.install.restart_required}
							<div class="flex items-start gap-2 rounded-md border border-advisory/35 bg-advisory-soft px-3 py-2 text-sm text-ink">
								<RotateCw class="mt-0.5 size-4 shrink-0 text-advisory" aria-hidden="true" />
								<p>{m.settings_packs_restart()}</p>
							</div>
						{/if}
						{#if applied.kind && applied.install.pack.enabled}
							<div>
								<Button size="sm" variant="secondary" href={`/targets/new?kind=${encodeURIComponent(applied.kind)}`}>
									<Plus class="size-4" aria-hidden="true" />
									{m.settings_packs_add_device({ label: applied.install.pack.label })}
								</Button>
							</div>
						{/if}
					</div>
				{/if}
			</section>
		{/if}
	</div>
</Disclosure>
