<script lang="ts">
	/**
	 * Settings → System → Modules: the optional parts of the console (wall
	 * mode, SLA reports, status pages, SLO, threat map). Each is a switch that
	 * saves on its own. Off, a module keeps its data but disappears from the
	 * navigation, the settings and the pages, and its API answers "not found".
	 */
	import type { Features, FeaturesPatch } from '#lib/api/types.js';
	import { m } from '#lib/paraglide/messages.js';
	import { auth } from '#lib/stores/auth.svelte.js';
	import { featuresStore } from '#lib/stores/features.svelte.js';
	import { ErrorNotice, Panel, Plate, Skeleton, Toggle } from '#lib/ui/index.js';

	const MODULES: { key: keyof Features; title: () => string; text: () => string }[] = [
		{ key: 'wall', title: () => m.settings_modules_wall_title(), text: () => m.settings_modules_wall_text() },
		{ key: 'sla_reports', title: () => m.settings_modules_sla_title(), text: () => m.settings_modules_sla_text() },
		{ key: 'status_pages', title: () => m.settings_modules_status_title(), text: () => m.settings_modules_status_text() },
		{ key: 'slo', title: () => m.settings_modules_slo_title(), text: () => m.settings_modules_slo_text() },
		{ key: 'threat_map', title: () => m.settings_modules_threat_title(), text: () => m.settings_modules_threat_text() }
	];

	let busy = $state<keyof Features | null>(null);
	let error = $state<unknown>(null);
	let saved = $state<keyof Features | null>(null);

	$effect(() => {
		void featuresStore.load();
	});

	async function set(key: keyof Features, value: boolean) {
		if (busy || !auth.isAdmin) return;
		busy = key;
		error = null;
		saved = null;
		try {
			const patch: FeaturesPatch = {};
			patch[key] = value;
			await featuresStore.update(patch);
			saved = key;
		} catch (cause) {
			error = cause;
		} finally {
			busy = null;
		}
	}
</script>

<Panel id="modules" title={m.settings_modules_title()} description={m.settings_modules_text()}>
	{#if featuresStore.data}
		<ul class="m-0 list-none divide-y divide-line p-0">
			{#each MODULES as module (module.key)}
				<li class="flex items-start gap-3 py-3 first:pt-0 last:pb-0">
					<Toggle
						id={`settings-module-${module.key}`}
						checked={featuresStore.data[module.key]}
						disabled={!auth.isAdmin || busy !== null}
						label={module.title()}
						onchange={(value) => void set(module.key, value)}
					/>
					<div class="min-w-0 flex-1">
						<label for={`settings-module-${module.key}`} class="block text-sm font-semibold text-ink">{module.title()}</label>
						<p class="m-0 mt-0.5 text-[0.8125rem] text-ink-2">{module.text()}</p>
					</div>
					{#if saved === module.key}<Plate tone="signal" draw>{m.settings_general_saved()}</Plate>{/if}
				</li>
			{/each}
		</ul>
		{#if error}<div class="mt-3"><ErrorNotice {error} /></div>{/if}
	{:else if featuresStore.loaded}
		<p class="m-0 text-sm text-ink-2">{m.settings_modules_unreadable()}</p>
	{:else}
		<Skeleton class="h-32" />
	{/if}
</Panel>
