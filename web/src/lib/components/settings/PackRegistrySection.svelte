<script lang="ts">
	/**
	 * Settings → Integration packs → Community registry. The administrator
	 * sets the address of a registry (an index.json); nothing is fetched while
	 * it is empty. The listed packs can be installed or updated: the server
	 * checks the checksum and the licence before installing.
	 *
	 * Errors and the index's own texts come from the server and are shown as
	 * served.
	 */
	import { ExternalLink, RotateCw } from 'lucide-svelte';
	import {
		getPackRegistry,
		installRegistryPack,
		PACK_REGISTRY_DOC_URL,
		setPackRegistry,
		type RegistryPack,
		type RegistryView
	} from '#lib/api/packs.js';
	import type { PackView } from '#lib/api/index.js';
	import { Button, Disclosure, ErrorNotice, Field, Plate } from '#lib/ui/index.js';
	import { m } from '#lib/paraglide/messages.js';
	import { withBase } from '#lib/paths.js';
	import { OFFICIAL_REGISTRY_URL, filterRegistryPacks } from './pack-registry.js';

	interface Props {
		/** Called with the installed pack, so the list above can show it. */
		oninstalled?: (pack: PackView) => void;
		/**
		 * Shown inside a dialog (Add monitor): no disclosure, no address form
		 * (a link leads to Settings), a search box over the packs.
		 */
		embedded?: boolean;
	}

	let { oninstalled, embedded = false }: Props = $props();

	let view = $state<RegistryView | null>(null);
	let address = $state('');
	let loading = $state(false);
	let saving = $state(false);
	let loadError = $state<unknown>(null);
	let busy = $state<string | null>(null);
	let rowError = $state<{ id: string; error: unknown } | null>(null);

	async function load() {
		loading = true;
		loadError = null;
		try {
			view = await getPackRegistry();
			address = view.url;
		} catch (cause) {
			loadError = cause;
		} finally {
			loading = false;
		}
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		await saveAddress(address.trim());
	}

	/** Sets the address of the official registry, then reads it. */
	async function useOfficial() {
		address = OFFICIAL_REGISTRY_URL;
		await saveAddress(OFFICIAL_REGISTRY_URL);
	}

	async function saveAddress(url: string) {
		saving = true;
		loadError = null;
		try {
			await setPackRegistry(url);
			await load();
		} catch (cause) {
			loadError = cause;
		} finally {
			saving = false;
		}
	}

	async function install(pack: RegistryPack) {
		busy = pack.id;
		rowError = null;
		try {
			const report = await installRegistryPack(pack.id);
			oninstalled?.(report.pack);
			await load();
		} catch (cause) {
			rowError = { id: pack.id, error: cause };
		} finally {
			busy = null;
		}
	}

	let opened = $state(false);
	let query = $state('');
	const listed = $derived(view ? filterRegistryPacks(view.packs, query) : []);
	$effect(() => {
		if ((embedded || opened) && !view && !loading) void load();
	});
</script>

{#snippet official()}
	<div class="grid gap-3 rounded-lg border border-line bg-surface-2 p-3">
		<p class="max-w-[68ch] text-sm text-ink-2">{m.settings_packs_registry_off_hint()}</p>
		<div class="flex flex-wrap items-center gap-2">
			<Button variant="primary" loading={saving} onclick={() => void useOfficial()}>
				{m.settings_packs_registry_use_official()}
			</Button>
			{#if embedded}
				<a
					class="inline-flex min-h-10 items-center text-sm font-semibold text-signal underline-offset-2 hover:underline"
					href={withBase('/settings/monitoring#packs')}
				>
					{m.settings_packs_registry_other_address()}
				</a>
			{/if}
		</div>
	</div>
{/snippet}

{#snippet body()}
	<div class={embedded ? 'grid gap-4' : 'mt-4 grid gap-4'}>
		<p class="max-w-[68ch] text-sm text-ink-2">{m.settings_packs_registry_intro()}</p>

		{#if view && !view.configured}
			{@render official()}
		{/if}

		{#if !embedded}
		<form class="grid gap-3" onsubmit={save} novalidate>
			<Field label={m.settings_packs_registry_url_label()} for="pack-registry-url">
				<input
					id="pack-registry-url"
					class="input"
					type="url"
					inputmode="url"
					autocomplete="off"
					spellcheck="false"
					placeholder="https://example.github.io/packs/index.json"
					bind:value={address}
				/>
			</Field>
			<div class="flex flex-wrap items-center gap-2">
				<Button type="submit" variant="secondary" loading={saving}>{m.settings_packs_registry_save()}</Button>
				{#if view?.configured}
					<Button variant="ghost" onclick={load} loading={loading}>
						<RotateCw class="size-4" aria-hidden="true" />
						{m.settings_packs_registry_refresh()}
					</Button>
				{/if}
				<a
					class="inline-flex min-h-10 items-center gap-1.5 text-sm font-semibold text-signal underline-offset-2 hover:underline"
					href={PACK_REGISTRY_DOC_URL}
					target="_blank"
					rel="noopener noreferrer"
				>
					{m.settings_packs_registry_doc()}
					<ExternalLink class="size-3.5" aria-hidden="true" />
				</a>
			</div>
		</form>
		{:else if view?.configured}
			<p class="flex flex-wrap items-center gap-x-2 text-[0.8125rem] text-ink-2">
				<span class="min-w-0 font-mono break-all">{view.url}</span>
				<a class="inline-flex min-h-10 items-center font-semibold text-signal underline-offset-2 hover:underline" href={withBase('/settings/monitoring#packs')}>
					{m.settings_packs_registry_other_address()}
				</a>
			</p>
		{/if}

		{#if loadError}
			<ErrorNotice error={loadError} title={m.settings_packs_registry_error()} />
		{:else if view?.error}
			<p class="text-sm text-warning" role="alert">{view.error}</p>
		{/if}

		{#if view?.configured && !view.error}
			{#if view.packs.length === 0}
				<p class="text-sm text-ink-2">{m.settings_packs_registry_empty()}</p>
			{:else}
				{#if embedded}
					<Field label={m.settings_packs_registry_search()} for="pack-registry-search">
						<input id="pack-registry-search" class="input" type="search" autocomplete="off" bind:value={query} />
					</Field>
				{/if}
				{#if listed.length === 0}
					<p class="text-sm text-ink-2">{m.settings_packs_registry_no_match()}</p>
				{/if}
				<ul class="grid gap-2">
					{#each listed as pack (pack.id)}
						<li class="grid gap-2 rounded-lg border border-line bg-surface p-3 sm:grid-cols-[1fr_auto] sm:items-center">
							<div class="min-w-0">
								<p class="font-semibold text-ink">
									{pack.name}
									<span class="font-mono text-[0.8125rem] font-normal text-ink-2">{pack.id}</span>
								</p>
								{#if pack.description}
									<p class="text-sm text-ink-2">{pack.description}</p>
								{/if}
								<p class="text-[0.8125rem] text-ink-2">
									{m.settings_packs_registry_meta({
										author: pack.author || '-',
										license: pack.license,
										version: pack.version
									})}
								</p>
								{#if rowError?.id === pack.id}
									<ErrorNotice error={rowError.error} title={m.settings_packs_registry_install_error()} />
								{/if}
							</div>
							<div class="flex items-center gap-2">
								{#if pack.status === 'installed'}
									<Plate tone="signal" label={m.settings_packs_registry_installed()} />
								{:else if pack.status === 'incompatible'}
									<Plate tone="warning" label={m.settings_packs_registry_incompatible()} />
								{:else}
									<Button
										size="sm"
										variant="secondary"
										loading={busy === pack.id}
										disabled={busy !== null}
										onclick={() => install(pack)}
									>
										{pack.status === 'update'
											? m.settings_packs_registry_update({ version: pack.version })
											: m.settings_packs_registry_install()}
									</Button>
								{/if}
							</div>
						</li>
					{/each}
				</ul>
			{/if}
		{/if}
	</div>
{/snippet}

{#if embedded}
	{@render body()}
{:else}
	<Disclosure title={m.settings_packs_registry_title()} bind:open={opened}>
		{@render body()}
	</Disclosure>
{/if}
