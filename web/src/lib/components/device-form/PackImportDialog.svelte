<script lang="ts">
	/**
	 * "Zabbix or snmp_exporter template" and "Community packs" of the Add monitor
	 * picker: a modal dialog around the same sections as Settings → Integration
	 * packs (no logic of their own here). When a pack is installed, the page is
	 * told so it can read the catalogue again and show the new kinds. Admin
	 * only: the caller decides whether to render it.
	 */
	import { tick } from 'svelte';
	import { X } from 'lucide-svelte';
	import { m } from '#lib/paraglide/messages.js';
	import type { PackView } from '#lib/api/index.js';
	import { DocLink } from '#lib/ui/index.js';
	import TemplateImportSection from '#lib/components/settings/TemplateImportSection.svelte';
	import PackRegistrySection from '#lib/components/settings/PackRegistrySection.svelte';

	interface Props {
		mode: 'template' | 'registry';
		/** Called after a pack was installed or updated, so the page can reload its catalogue. */
		oninstalled?: (pack: PackView) => void;
	}
	let { mode, oninstalled }: Props = $props();

	let open = $state(false);
	let dialog = $state<HTMLElement | null>(null);
	let returnFocus: HTMLElement | null = null;

	const title = $derived(mode === 'template' ? m.deviceform_packs_template_title() : m.deviceform_packs_registry_title());

	export function show() {
		returnFocus = (document.activeElement as HTMLElement | null) ?? null;
		open = true;
		void tick().then(() => dialog?.focus());
	}

	function close() {
		open = false;
		returnFocus?.focus();
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			event.preventDefault();
			close();
		} else if (event.key === 'Tab' && dialog) {
			const items = [
				...dialog.querySelectorAll<HTMLElement>('a[href], button:not([disabled]), input:not([disabled]), textarea:not([disabled])')
			].filter((el) => el.offsetParent !== null);
			if (items.length === 0) return;
			const first = items[0];
			const final = items[items.length - 1];
			const active = document.activeElement;
			if (event.shiftKey && (active === first || active === dialog)) {
				event.preventDefault();
				final.focus();
			} else if (!event.shiftKey && active === final) {
				event.preventDefault();
				first.focus();
			}
		}
	}
</script>

{#if open}
	<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
	<div class="fixed inset-0 z-[60] bg-black/50" aria-hidden="true" onclick={close}></div>
	<div
		bind:this={dialog}
		role="dialog"
		aria-modal="true"
		aria-labelledby={`pack-import-title-${mode}`}
		tabindex="-1"
		{onkeydown}
		class="fixed inset-x-0 bottom-0 z-[62] max-h-[92dvh] overflow-y-auto rounded-t-[var(--radius-card)] border border-line bg-surface p-5 pb-[calc(1.25rem+var(--safe-bottom))] text-ink shadow-float outline-none sm:inset-x-auto sm:top-1/2 sm:bottom-auto sm:left-1/2 sm:w-[48rem] sm:max-w-[calc(100vw-2rem)] sm:-translate-x-1/2 sm:-translate-y-1/2 sm:rounded-[var(--radius-card)] sm:pb-5"
	>
		<div class="flex items-start justify-between gap-3">
			<div class="min-w-0">
				<h2 id={`pack-import-title-${mode}`} class="text-lg font-semibold">{title}</h2>
				<DocLink page={mode === 'template' ? 'integrations/template-import/' : 'packs/registry/'} class="mt-0.5" />
			</div>
			<button
				type="button"
				class="-mt-1.5 -mr-2 inline-flex size-10 shrink-0 items-center justify-center rounded-md text-ink-3 hover:bg-surface-2 hover:text-ink"
				aria-label={m.devices_import_close()}
				onclick={close}
			>
				<X class="size-4" aria-hidden="true" />
			</button>
		</div>
		<div class="mt-4">
			{#if mode === 'template'}
				<TemplateImportSection embedded {oninstalled} />
			{:else}
				<PackRegistrySection embedded {oninstalled} />
			{/if}
		</div>
	</div>
{/if}
