<script lang="ts">
	/**
	 * Setup wizard, step "Modules": the optional parts of the app, each with a
	 * switch and one line on what it does. All start off; the page saves them
	 * on Continue (`PUT /api/settings/features`).
	 */
	import { FileText, Globe, Monitor, ShieldAlert, Target } from 'lucide-svelte';
	import type { Features } from '#lib/api/index.js';
	import { m } from '#lib/paraglide/messages.js';
	import { Toggle } from '#lib/ui/index.js';

	interface Props {
		value: Features;
		disabled?: boolean;
	}
	let { value = $bindable(), disabled = false }: Props = $props();

	const MODULES: { key: keyof Features; icon: typeof Monitor; name: () => string; text: () => string }[] = [
		{ key: 'wall', icon: Monitor, name: () => m.wizard_modules_wall(), text: () => m.wizard_modules_wall_text() },
		{ key: 'sla_reports', icon: FileText, name: () => m.wizard_modules_sla_reports(), text: () => m.wizard_modules_sla_reports_text() },
		{ key: 'status_pages', icon: Globe, name: () => m.wizard_modules_status_pages(), text: () => m.wizard_modules_status_pages_text() },
		{ key: 'slo', icon: Target, name: () => m.wizard_modules_slo(), text: () => m.wizard_modules_slo_text() },
		{ key: 'threat_map', icon: ShieldAlert, name: () => m.wizard_modules_threat_map(), text: () => m.wizard_modules_threat_map_text() }
	];
</script>

<ul class="divide-y divide-line rounded-[var(--radius-card)] border border-line bg-surface">
	{#each MODULES as module (module.key)}
		{@const Icon = module.icon}
		<li class="flex min-w-0 items-center gap-4 px-4 py-4 sm:px-5">
			<Icon class="size-5 shrink-0 text-ink-2" aria-hidden="true" />
			<div class="min-w-0 flex-1">
				<label for={`wizard-module-${module.key}`} class="block font-semibold break-words text-ink">{module.name()}</label>
				<p class="mt-0.5 text-[0.8125rem] break-words text-ink-2">{module.text()}</p>
			</div>
			<Toggle id={`wizard-module-${module.key}`} label={module.name()} {disabled} bind:checked={value[module.key]} />
		</li>
	{/each}
</ul>
