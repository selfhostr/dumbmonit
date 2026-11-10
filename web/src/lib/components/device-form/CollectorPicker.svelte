<script lang="ts">
	import { m } from '#lib/paraglide/messages.js';
	/**
	 * Step 1 of adding anything: what do you want to watch?
	 *
	 * A search box with filters (category, no login needed, beta, community
	 * packs: all combine with the text), a two-line legend for the badges, then
	 * every kind under its category. Every kind comes from `GET /api/collectors`;
	 * only icons, categories and search words are ours. The agent is one choice:
	 * what it reports is found by search and detected once it runs.
	 * Arrow keys move between choices, Space/Enter select.
	 */
	import { Check, Radar, Search, Upload, X } from 'lucide-svelte';
	import type { CollectorInfo } from '#lib/api/index.js';
	import { Chip, DocLink, Plate } from '#lib/ui/index.js';
	import KindIcon from '#lib/components/KindIcon.svelte';
	import { FACETS, passesFilters, type Facet } from './categories';
	import { CATEGORY_ICON, categoryTitle, groupCollectors, matchesQuery, type ImportChoice, type KindChoice } from './kinds';

	interface Props {
		collectors: CollectorInfo[];
		/** Listed in their own "Import" category, last. */
		imports?: ImportChoice[];
		/** Id of the current choice: a kind, or `agent:<feature>`. */
		selected: string | null;
		onselect: (kind: string, feature: string | null) => void;
		/** Three columns on large screens, when nothing sits beside the picker. */
		wide?: boolean;
		/** Adds a "Scan the network" shortcut at the head of the network category. */
		onscan?: () => void;
		/** Shown when nothing matches: a way out. */
		empty?: import('svelte').Snippet<[string]>;
	}

	let { collectors, imports = [], selected, onselect, onscan, wide = false, empty }: Props = $props();

	let query = $state('');
	let categories = $state<Set<string>>(new Set());
	let facets = $state<Set<Facet>>(new Set());

	const groups = $derived(groupCollectors(collectors));
	const all = $derived(groups.flatMap((g) => g.choices));
	const hasBeta = $derived(all.some((c) => c.beta));
	const hasPack = $derived(all.some((c) => c.pack));
	const searching = $derived(query.trim().length > 0);
	const filtering = $derived(categories.size > 0 || facets.size > 0);
	const fold = (text: string) => text.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase();

	/** Matching the text and the facets, whatever the categories: what the chips count. */
	const inScope = $derived(all.filter((c) => matchesQuery(c, query) && passesFilters(c, new Set(), facets)));
	const importScope = $derived(
		facets.size > 0 ? [] : imports.filter((i) => !searching || fold(`${i.label} ${i.summary}`).includes(fold(query.trim())))
	);
	const counts = $derived.by(() => {
		const n = new Map<string, number>();
		for (const c of inScope) n.set(c.category, (n.get(c.category) ?? 0) + 1);
		if (importScope.length > 0) n.set('import', importScope.length);
		return n;
	});

	const sections = $derived(
		groups
			.map((g) => ({ ...g, choices: g.choices.filter((c) => inScope.includes(c) && passesFilters(c, categories, new Set())) }))
			.filter((g) => g.choices.length > 0)
	);
	const showImports = $derived(importScope.length > 0 && (categories.size === 0 || categories.has('import')));
	const flat = $derived<KindChoice[]>(sections.flatMap((g) => g.choices));
	/** The scan shortcut leads the network category when nothing narrows the list. */
	const showScan = $derived(!!onscan && !searching && facets.size === 0 && (categories.size === 0 || categories.has('network')));
	/** Roving tabindex: the selected choice, or the first one, is the tab stop. */
	const tabStop = $derived(selected && flat.some((c) => c.id === selected) ? selected : flat[0]?.id);

	function toggle<T>(set: Set<T>, value: T): Set<T> {
		const next = new Set(set);
		if (next.has(value)) next.delete(value);
		else next.add(value);
		return next;
	}
	function clearFilters() {
		categories = new Set();
		facets = new Set();
	}

	let host = $state<HTMLDivElement | null>(null);

	function choose(id: string) {
		const choice = flat.find((c) => c.id === id);
		if (choice) onselect(choice.kind, choice.feature);
	}

	function move(from: string, delta: number) {
		const index = flat.findIndex((c) => c.id === from);
		if (index === -1 || flat.length === 0) return;
		const next = flat[(index + delta + flat.length) % flat.length];
		// Moving only moves the focus: selecting would scroll to the form each time.
		host?.querySelector<HTMLButtonElement>(`[data-choice="${next.id}"]`)?.focus();
	}

	function onkeydown(event: KeyboardEvent, id: string) {
		switch (event.key) {
			case 'ArrowRight':
			case 'ArrowDown':
				event.preventDefault();
				move(id, 1);
				break;
			case 'ArrowLeft':
			case 'ArrowUp':
				event.preventDefault();
				move(id, -1);
				break;
			case ' ':
			case 'Enter':
				event.preventDefault();
				choose(id);
				break;
		}
	}

	/** Enter in the search box takes the only, or first, match. */
	function onsearchkey(event: KeyboardEvent) {
		if (event.key === 'Enter' && query.trim() && flat.length > 0) {
			event.preventDefault();
			choose(flat[0].id);
		} else if (event.key === 'ArrowDown' && flat.length > 0) {
			event.preventDefault();
			host?.querySelector<HTMLButtonElement>(`[data-choice="${flat[0].id}"]`)?.focus();
		}
	}
</script>

<div class="grid gap-4">
	<div class="relative">
		<Search class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink-3" aria-hidden="true" />
		<input
			type="search"
			class="input"
			style="padding-left: 2.25rem"
			placeholder={m.deviceform_picker_search_placeholder()}
			bind:value={query}
			onkeydown={onsearchkey}
			autocomplete="off"
			spellcheck="false"
			aria-label={m.deviceform_picker_search_label()}
		/>
	</div>

	<div class="grid gap-2">
		<div
			role="group"
			aria-label={m.deviceform_picker_categories_label()}
			class="-mx-1 flex gap-2 overflow-x-auto px-1 pb-1 sm:flex-wrap sm:overflow-visible sm:pb-0"
		>
			{#each groups as group (group.id)}
				{@const Icon = group.icon}
				{@const n = counts.get(group.id) ?? 0}
				<Chip pressed={categories.has(group.id)} disabled={n === 0 && !categories.has(group.id)} count={n} onclick={() => (categories = toggle(categories, group.id))}>
					<Icon class="size-4 shrink-0" aria-hidden="true" />
					{group.title}
				</Chip>
			{/each}
			{#if imports.length > 0}
				{@const n = counts.get('import') ?? 0}
				<Chip pressed={categories.has('import')} disabled={n === 0 && !categories.has('import')} count={n} onclick={() => (categories = toggle(categories, 'import'))}>
					<Upload class="size-4 shrink-0" aria-hidden="true" />
					{m.deviceform_picker_import()}
				</Chip>
			{/if}
		</div>
		<div role="group" aria-label={m.deviceform_picker_facets_label()} class="flex flex-wrap items-center gap-2">
			{#each FACETS as facet (facet)}
				{#if facet === 'nologin' || (facet === 'beta' && hasBeta) || (facet === 'pack' && hasPack)}
					<Chip pressed={facets.has(facet)} onclick={() => (facets = toggle(facets, facet))}>
						{facet === 'nologin' ? m.deviceform_picker_facet_nologin() : facet === 'beta' ? m.deviceform_picker_facet_beta() : m.deviceform_picker_facet_pack()}
					</Chip>
				{/if}
			{/each}
			{#if filtering}
				<button
					type="button"
					class="inline-flex min-h-10 items-center gap-1 rounded px-2 text-[0.8125rem] font-medium text-ink-2 hover:text-ink hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal"
					onclick={clearFilters}
				>
					<X class="size-3.5" aria-hidden="true" />
					{m.deviceform_picker_clear()}
				</button>
			{/if}
		</div>
	</div>

	<ul class="grid gap-1 text-[0.8125rem] leading-snug text-ink-2">
		{#if hasBeta}
			<li class="flex items-start gap-2">
				<Plate tone="advisory" bare label={m.deviceform_picker_beta()} class="mt-px shrink-0" />
				<span>{m.deviceform_picker_beta_note()}</span>
			</li>
		{/if}
		<li class="flex items-start gap-2">
			<Plate tone="info" bare label={m.deviceform_picker_pack()} class="mt-px shrink-0" />
			<span class="grid">
				{m.deviceform_picker_pack_note()}
				<DocLink page="packs/" label={m.deviceform_picker_pack_learn()} />
			</span>
		</li>
	</ul>

	{#if flat.length === 0 && !showImports}
		<div class="rounded-[var(--radius-card)] border border-dashed border-line px-4 py-6 text-center">
			<p class="text-sm text-ink">{searching ? m.deviceform_picker_no_match({ query: query.trim() }) : m.deviceform_picker_no_match_filters()}</p>
			{#if filtering}
				<button type="button" class="mt-2 min-h-10 text-sm font-medium text-signal-ink hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal" onclick={clearFilters}>
					{m.deviceform_picker_clear()}
				</button>
			{/if}
			{#if empty && searching}{@render empty(query.trim())}{/if}
		</div>
	{:else}
		<div bind:this={host} role="radiogroup" aria-label={m.deviceform_picker_group_label()} class="grid gap-6">
			{#each sections as group (group.id)}
				{@const Icon = group.icon}
				<section aria-labelledby={`picker-cat-${group.id}`}>
					<h3 id={`picker-cat-${group.id}`} class="mb-2.5 flex items-center gap-2.5 text-[0.9375rem] font-semibold text-ink">
						<span class="flex size-7 shrink-0 items-center justify-center rounded-lg bg-surface-2 text-ink-2"><Icon class="size-4" aria-hidden="true" /></span>
						{group.title}
						<span class="text-[0.8125rem] font-normal tabular-nums text-ink-3">{group.choices.length}</span>
						<span class="h-px min-w-4 flex-1 bg-line" aria-hidden="true"></span>
					</h3>
					{@render tiles(group.choices, group.id === 'network' && showScan)}
				</section>
			{/each}
			{#if showImports}
				<section aria-labelledby="picker-cat-import">
					<h3 id="picker-cat-import" class="mb-2.5 flex items-center gap-2.5 text-[0.9375rem] font-semibold text-ink">
						<span class="flex size-7 shrink-0 items-center justify-center rounded-lg bg-surface-2 text-ink-2"><Upload class="size-4" aria-hidden="true" /></span>
						{m.deviceform_picker_import()}
						<span class="text-[0.8125rem] font-normal tabular-nums text-ink-3">{importScope.length}</span>
						<span class="h-px min-w-4 flex-1 bg-line" aria-hidden="true"></span>
					</h3>
					<p class="mb-2 text-[0.8125rem] text-ink-3">{m.deviceform_picker_import_hint()}</p>
					{@render importTiles(importScope)}
				</section>
			{/if}
		</div>
	{/if}
</div>

{#snippet tiles(items: KindChoice[], withScan = false)}
	<ul class={`grid gap-2 sm:grid-cols-2 ${wide ? 'xl:grid-cols-3' : ''}`}>
		{#if withScan && onscan}
			<li class="min-w-0">
				<button
					type="button"
					data-choice="scan"
					onclick={onscan}
					class="flex h-full w-full items-start gap-3 rounded-[var(--radius-card)] border border-line bg-surface px-3.5 py-3 text-left transition-[transform,box-shadow,border-color] duration-200 ease-out-expo hover:-translate-y-px hover:border-line-strong hover:shadow-float focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal"
				>
					<span class="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg border border-line bg-surface-2 text-ink-2">
						<Radar class="size-4" aria-hidden="true" />
					</span>
					<span class="min-w-0 flex-1">
						<span class="text-[0.9375rem] leading-tight font-semibold text-ink">{m.deviceform_new_scan()}</span>
						<span class="mt-1 line-clamp-2 block text-[0.8125rem] leading-snug text-ink-2">{m.deviceform_new_door_scan_desc()}</span>
					</span>
				</button>
			</li>
		{/if}
		{#each items as choice, i (choice.id)}
			{@const Icon = choice.icon}
			{@const active = choice.id === selected}
			<li class="min-w-0" style={`--rise-delay: ${Math.min(i, 10) * 25}ms`}>
				<button
					type="button"
					role="radio"
					aria-checked={active}
					tabindex={choice.id === tabStop ? 0 : -1}
					data-choice={choice.id}
					data-kind={choice.feature ? undefined : choice.kind}
					onclick={() => onselect(choice.kind, choice.feature)}
					onkeydown={(event) => onkeydown(event, choice.id)}
					class={`relative flex h-full w-full items-start gap-3 rounded-[var(--radius-card)] border px-3.5 py-3 text-left transition-[transform,box-shadow,border-color,background-color] duration-200 ease-out-expo focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal ${
						active
							? 'border-signal bg-signal-soft shadow-lift'
							: 'border-line bg-surface hover:-translate-y-px hover:border-line-strong hover:shadow-float'
					}`}
				>
					<span
						class={`mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg border ${
							active ? 'border-signal/30 bg-surface text-signal-ink' : 'border-line bg-surface-2 text-ink-2'
						}`}
					>
						{#if choice.feature}
							<Icon class="size-4" aria-hidden="true" />
						{:else}
							<KindIcon kind={choice.kind} />
						{/if}
					</span>
					<span class="min-w-0 flex-1 pr-5">
						<span class="flex flex-wrap items-center gap-x-2 gap-y-1">
							<span class="text-[0.9375rem] leading-tight font-semibold text-ink">{choice.label}</span>
							{#if choice.beta}<Plate tone="advisory" bare label={m.deviceform_picker_beta()} title={m.deviceform_picker_beta_title()} />{/if}
							{#if choice.pack}<Plate tone="info" bare label={m.deviceform_picker_pack()} title={m.deviceform_picker_pack_title()} />{/if}
						</span>
						{#if choice.summary}
							<span class="mt-1 line-clamp-2 text-[0.8125rem] leading-snug text-ink-2">{choice.summary}</span>
						{/if}
					</span>
					{#if active}
						<span class="absolute top-2.5 right-2.5 flex size-5 items-center justify-center rounded-full bg-signal text-on-signal" aria-hidden="true">
							<Check class="size-3.5" />
						</span>
					{/if}
				</button>
			</li>
		{/each}
	</ul>
{/snippet}

{#snippet importTiles(items: ImportChoice[])}
	<ul class={`grid gap-2 sm:grid-cols-2 ${wide ? 'xl:grid-cols-3' : ''}`}>
		{#each items as item, i (item.id)}
			{@const Icon = item.icon}
			<li class="min-w-0" style={`--rise-delay: ${Math.min(i, 10) * 25}ms`}>
				<button
					type="button"
					data-choice={`import:${item.id}`}
					onclick={item.onrun}
					aria-haspopup="dialog"
					class="flex h-full w-full items-start gap-3 rounded-[var(--radius-card)] border border-line bg-surface px-3.5 py-3 text-left transition-[transform,box-shadow,border-color] duration-200 ease-out-expo hover:-translate-y-px hover:border-line-strong hover:shadow-float focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal"
				>
					<span class="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg border border-line bg-surface-2 text-ink-2">
						<Icon class="size-4" aria-hidden="true" />
					</span>
					<span class="min-w-0 flex-1">
						<span class="text-[0.9375rem] leading-tight font-semibold text-ink">{item.label}</span>
						<span class="mt-1 line-clamp-2 block text-[0.8125rem] leading-snug text-ink-2">{item.summary}</span>
					</span>
				</button>
			</li>
		{/each}
	</ul>
{/snippet}
