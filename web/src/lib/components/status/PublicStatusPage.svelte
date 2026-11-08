<script lang="ts">
	import { withBase } from '#lib/paths.js';
	/**
	 * Public status page. Rendered by `/s/<slug>`, and by the root layout at `/`
	 * when the request came in on the page's own public domain. No session, no
	 * nav, no cookie: one document from `GET /api/public/status/<slug>`,
	 * refreshed every minute.
	 *
	 * Top to bottom: logo, title and the overall banner, the open announcements
	 * (incidents, maintenance) with their timeline, the services by group with
	 * their daily history bar, "Past incidents" by day for 30 days, then how to
	 * get updates (RSS, and email when the owner set up an SMTP channel). The
	 * theme follows the page setting (`light` / `dark`) or the visitor's system;
	 * the accent is one of a closed set of tokens, never free-form CSS.
	 */
	import VersionTag from '#lib/components/VersionTag.svelte';
	import { m } from '#lib/paraglide/messages.js';
	import { getLocale } from '#lib/paraglide/runtime.js';
	import { CalendarClock, ExternalLink, Megaphone, Moon, Sun } from 'lucide-svelte';
	import { getPublicStatus, getPublicStatusDay, toApiError, type PublicDayDetail, type PublicIncident, type PublicStatus } from '#lib/api/index.js';
	import { formatDateTime, formatPercent, formatRelative, parseServerDate } from '#lib/format.js';
	import { theme } from '#lib/stores/theme.svelte.js';
	import { DecryptText, EmptyState, ErrorNotice, Plate, Skeleton } from '#lib/ui/index.js';
	import Logo from '#lib/components/Logo.svelte';
	import AccessGate from '#lib/components/status/AccessGate.svelte';
	import IncidentCard from '#lib/components/status/IncidentCard.svelte';
	import StatusUpdates from '#lib/components/status/StatusUpdates.svelte';
	import CalendarHistory from '#lib/components/status/CalendarHistory.svelte';
	import UptimeLegend from '#lib/components/status/UptimeLegend.svelte';
	import ServiceRow from '#lib/components/status/ServiceRow.svelte';
	import DayDetail from '#lib/components/status/DayDetail.svelte';
	import { periodParts } from '#lib/components/status/period.js';
	import StatusMascot from '#lib/components/status/StatusMascot.svelte';
	import { tallyItems } from '#lib/components/status/tally.js';
	import { accentClass, homepageHost, isClosed, overallBanner } from '#lib/components/status/words.js';

	const REFRESH_MS = 60_000;

	interface Props {
		slug: string;
	}

	let { slug }: Props = $props();

	let status = $state<PublicStatus | null>(null);
	const seo = $derived(status?.page.seo);
	// The server wrote these tags into the HTML for crawlers; once the page is drawn
	// the <svelte:head> below owns them, so the copies are dropped (no duplicates).
	$effect(() => {
		if (seo) document.head.querySelectorAll('[data-dm-seo]').forEach((node) => node.remove());
	});
	let loading = $state(true);
	let error = $state<unknown>(null);
	let lastChecked = $state<Date | null>(null);

	async function load(signal?: AbortSignal) {
		try {
			status = await getPublicStatus(slug, signal);
			error = null;
			lastChecked = new Date();
		} catch (cause) {
			if (cause instanceof DOMException && cause.name === 'AbortError') return;
			// A failed refresh keeps the last good document on screen.
			if (!status) error = cause;
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		const current = slug;
		if (!current) return;
		const controller = new AbortController();
		loading = true;
		status = null;
		void load(controller.signal);
		const timer = setInterval(() => void load(controller.signal), REFRESH_MS);
		return () => {
			controller.abort();
			clearInterval(timer);
		};
	});

	// The page decides its light: a fixed theme overrides whatever the visitor
	// (or the admin, on this browser) chose; `auto` follows the system.
	$effect(() => {
		// Simple mode ignores the page's fixed theme: the visitor's system (or toggle) decides.
		const wanted = status?.page.simple ? 'auto' : (status?.page.theme ?? 'auto');
		const dark = wanted === 'auto' ? theme.resolved === 'dark' : wanted === 'dark';
		document.documentElement.classList.toggle('dark', dark);
		return () => theme.apply();
	});

	const notFound = $derived(error !== null && toApiError(error).status === 404);
	// A private page answers 401 and says what it wants: a password or a DumbMonit sign-in.
	const gate = $derived(error !== null ? toApiError(error).access : null);
	const banner = $derived(status ? overallBanner(status) : null);
	const days = $derived(status?.page.history_days ?? 7);
	const windowSecs = $derived(status?.page.window_secs ?? days * 86400);
	const stepSecs = $derived(status?.page.step_secs ?? 86400);
	const periodLabel = $derived.by(() => {
		const part = periodParts(windowSecs);
		if (part.unit === 'hours') return part.count === 1 ? m.status_period_hour() : m.status_period_hours({ count: part.count });
		return m.status_period_days({ count: part.count });
	});

	// Hour-by-hour detail of the day whose bar was opened.
	let openDay = $state<string | null>(null);
	let dayDetail = $state<PublicDayDetail | null>(null);
	let dayLoading = $state(false);
	let dayError = $state<unknown>(null);

	async function showDay(date: string) {
		openDay = date;
		dayDetail = null;
		dayError = null;
		dayLoading = true;
		try {
			const detail = await getPublicStatusDay(slug, date);
			if (openDay === date) dayDetail = detail;
		} catch (cause) {
			if (openDay === date) dayError = cause;
		} finally {
			if (openDay === date) dayLoading = false;
		}
	}
	const bannerTone = $derived(banner?.tone ?? 'signal');

	// Open announcements sit at the top; everything closed goes to the history.
	const active = $derived<PublicIncident[]>(
		status ? [...status.maintenance, ...status.incidents].filter((i) => !isClosed(i.status)) : []
	);
	const past = $derived<PublicIncident[]>(
		status ? [...status.incidents, ...status.maintenance].filter((i) => isClosed(i.status)) : []
	);

	// "Past incidents" grouped by the day they started, newest day first.
	const pastByDay = $derived.by(() => {
		const dayFormat = new Intl.DateTimeFormat(getLocale(), { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
		const groups = new Map<string, { label: string; incidents: PublicIncident[] }>();
		for (const incident of past) {
			const date = parseServerDate(incident.starts_at);
			const key = date ? date.toISOString().slice(0, 10) : incident.starts_at;
			const label = date ? dayFormat.format(date) : incident.starts_at;
			const group = groups.get(key) ?? { label, incidents: [] };
			group.incidents.push(incident);
			groups.set(key, group);
		}
		return [...groups.entries()].sort((a, b) => (a[0] < b[0] ? 1 : -1)).map(([, group]) => group);
	});

	/** Services a subscriber may follow, by public key. */
	const subscribeServices = $derived(status ? status.groups.flatMap((g) => g.items.map((item) => ({ key: item.key, label: item.label }))) : []);
	const serviceCount = $derived(status ? status.groups.reduce((n, g) => n + g.items.length, 0) : 0);

	// Simple mode: no pigeon, no animation, only a light/dark toggle.
	const simple = $derived(status?.page.simple ?? false);

	// One line above the services: counts by state and the mean uptime over the bars' days.
	const tally = $derived.by(() => {
		const items = status ? status.groups.flatMap((g) => g.items) : [];
		const values = items.map((i) => i.uptime_history).filter((v): v is number => v !== null);
		const { up, degraded, down, maintenance, known } = tallyItems(items);
		const affected = degraded + down + maintenance;
		// "All N operational" counts only the services that were measured.
		let headline = m.status_public_tally_all({ count: known });
		if (known === 1 && affected === 0) headline = m.status_public_tally_one();
		if (known === 0) headline = m.status_words_overall_no_data();
		if (affected > 0) {
			const args = { affected, total: known };
			const only = [
				[down, () => m.status_public_tally_down(args)],
				[degraded, () => m.status_public_tally_degraded(args)],
				[maintenance, () => m.status_public_tally_maintenance(args)]
			] as const;
			const present = only.filter(([n]) => n > 0);
			headline = present.length === 1 ? present[0][1]() : m.status_public_tally_affected(args);
		}
		return {
			total: items.length,
			headline,
			detail: values.length && known > 0
				? m.status_public_tally_detail_uptime({ up, down, uptime: formatPercent(values.reduce((a, b) => a + b, 0) / values.length), days })
				: m.status_public_tally_detail({ up, down })
		};
	});
</script>

<svelte:head>
	<title>{seo ? seo.title : status ? m.status_public_title_with_page({ title: status.page.title }) : m.status_public_title()}</title>
	<!-- Same values the server wrote into the HTML (crates/server/src/status_seo.rs); noindex until the page says otherwise. -->
	<meta name="robots" content={seo?.indexable ? 'index, follow' : 'noindex, nofollow'} />
	{#if seo}
		<meta name="description" content={seo.description} />
		{#if seo.keywords}<meta name="keywords" content={seo.keywords} />{/if}
		{#if seo.indexable && seo.canonical}<link rel="canonical" href={seo.canonical} />{/if}
		<meta property="og:type" content="website" />
		<meta property="og:title" content={seo.title} />
		<meta property="og:description" content={seo.description} />
		<meta property="og:site_name" content={seo.site_name} />
		{#if seo.canonical}<meta property="og:url" content={seo.canonical} />{/if}
		{#if seo.image}<meta property="og:image" content={seo.image} />{/if}
		<meta name="twitter:card" content={seo.image ? 'summary_large_image' : 'summary'} />
		<meta name="twitter:title" content={seo.title} />
		<meta name="twitter:description" content={seo.description} />
		{#if seo.image}<meta name="twitter:image" content={seo.image} />{/if}
	{/if}
</svelte:head>

{#snippet announcements()}
	{#if active.length > 0}
		<section class="rise-in mt-8" style="--rise-delay: 80ms" aria-labelledby="announcements">
			<h2 id="announcements" class="text-base font-semibold tracking-tight text-ink">
				{active.length === 1 ? m.status_public_announcement_one() : m.status_public_announcement_many()}
			</h2>
			<div class="mt-3 grid gap-3">
				{#each active as incident (incident.kind + incident.starts_at + incident.title)}
					<IncidentCard {incident} />
				{/each}
			</div>
		</section>
	{/if}
{/snippet}

{#snippet services()}
	{#if status}
		<section class="rise-in mt-8" style="--rise-delay: 120ms" aria-labelledby="services">
			<div class="flex items-baseline justify-between gap-3">
				<h2 id="services" class="text-base font-semibold tracking-tight text-ink">{m.status_public_services()}</h2>
				<p class="text-[0.8125rem] text-ink-2">{stepSecs < 86400 ? periodLabel : m.status_public_uptime_last({ days })}</p>
			</div>
			{#if serviceCount > 0}
				<div class="mt-1.5"><UptimeLegend /></div>
			{/if}
			{#if serviceCount === 0}
				<EmptyState class="mt-3" icon={Megaphone} title={m.status_public_no_service_title()} description={m.status_public_no_service_description()} />
			{:else}
				<div class="mt-3 grid gap-4">
					{#each status.groups as group (group.name)}
						<div class="rounded-[var(--radius-card)] border border-line bg-surface shadow-lift">
							{#if group.name}
								<h3 class="label-tape border-b border-line px-4 py-2.5 text-ink-2 sm:px-5">{group.name}</h3>
							{/if}
							<ul class="divide-y divide-line" role="list">
								{#each group.items as item (item.label)}
									<ServiceRow {item} {days} {windowSecs} {stepSecs} {periodLabel} onselectday={showDay} />
								{/each}
							</ul>
						</div>
					{/each}
				</div>
			{/if}
			{#if openDay}
				<DayDetail date={openDay} detail={dayDetail} loading={dayLoading} error={dayError} onclose={() => (openDay = null)} />
			{/if}
			{#if status.calendar && status.calendar.length > 0}
				<h2 class="mt-6 text-base font-semibold tracking-tight text-ink">{m.status_calendar_title({ days: status.calendar.length })}</h2>
				<div class="mt-3"><CalendarHistory days={status.calendar} /></div>
			{/if}
		</section>
	{/if}
{/snippet}

{#snippet pastIncidents()}
	<section class="rise-in mt-10" style="--rise-delay: 160ms" aria-labelledby="past">
		<h2 id="past" class="text-base font-semibold tracking-tight text-ink">{m.status_public_past()}</h2>
		<p class="mt-0.5 text-[0.8125rem] text-ink-2">{m.status_public_last_30()}</p>
		{#if pastByDay.length === 0}
			<EmptyState class="mt-3" icon={CalendarClock} title={m.status_public_no_incident()} tone="signal" />
		{:else}
			<div class="mt-3 grid gap-5">
				{#each pastByDay as day (day.label)}
					<div>
						<h3 class="graticule pb-1.5 text-sm font-semibold text-ink">{day.label}</h3>
						<div class="mt-2 grid gap-2">
							{#each day.incidents as incident (incident.kind + incident.starts_at + incident.title)}
								<IncidentCard {incident} compact />
							{/each}
						</div>
					</div>
				{/each}
			</div>
		{/if}
	</section>
{/snippet}

{#snippet pageFooter()}
	<footer class="mt-12 grid justify-items-center gap-3 text-center text-[0.8125rem] text-ink-2">
		{#if status?.page.footer_text}
			<p class="max-w-prose whitespace-pre-line">{status.page.footer_text}</p>
		{/if}
		<p class="flex items-center gap-2">
			<Logo class="size-5" />
			<span>{m.status_public_powered_by()} <a class="font-semibold text-ink underline decoration-line underline-offset-2 hover:decoration-ink" href="https://github.com/selfhostr/dumbmonit" rel="noreferrer">DumbMonit</a></span>
		</p>
		<VersionTag inline />
	</footer>
{/snippet}

<div class={`min-h-full bg-canvas text-ink border-t-4 border-accent ${accentClass(status?.page.accent)}`}>
	<main class="mx-auto w-full max-w-3xl px-4 pt-8 pb-16 sm:px-6 sm:pt-12">
		{#if error && notFound}
			<EmptyState mascot="dizzy" title={m.status_public_not_found_title()} description={m.status_public_not_found_description()} />
		{:else if error && gate}
			<AccessGate {slug} access={gate} onunlocked={() => { error = null; loading = true; void load(); }} />
		{:else if error}
			<ErrorNotice {error} title={m.status_public_load_error()} onretry={() => void load()} />
		{:else if loading || !status || !banner}
			<div class="grid gap-6" aria-busy="true" aria-label={m.status_public_loading()}>
				<Skeleton class="h-9 w-2/3" />
				<Skeleton class="h-20 w-full" />
				<Skeleton class="h-28 w-full" />
				<Skeleton class="h-28 w-full" />
			</div>
		{:else}
			<header class="rise-in flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
				<div class="flex min-w-0 items-center gap-4">
					{#if status.page.logo_url}
						<img src={status.page.logo_url} alt={status.page.brand_name ?? ''} class="size-12 shrink-0 rounded-lg object-contain sm:size-14" />
					{/if}
					<div class="min-w-0">
						{#if status.page.brand_name && status.page.brand_name !== status.page.title}
							<p class="text-sm font-semibold text-ink-2 [overflow-wrap:anywhere]">{status.page.brand_name}</p>
						{/if}
						<h1 class="display text-3xl break-words text-ink sm:text-4xl">{status.page.title}</h1>
						{#if status.page.description}
							<p class="mt-1.5 max-w-prose text-base text-ink-2">{status.page.description}</p>
						{/if}
					</div>
				</div>
				{#if status.page.homepage_url}
					<a
						class="inline-flex min-h-10 items-center gap-1.5 rounded-md py-1 text-sm font-semibold text-accent underline decoration-accent/40 underline-offset-4 hover:decoration-accent"
						href={withBase(status.page.homepage_url)}
						rel="noopener noreferrer nofollow"
					>
						{homepageHost(status.page.homepage_url)}
						<ExternalLink class="size-3.5" aria-hidden="true" />
					</a>
				{/if}
				{#if simple}
					<button
						type="button"
						class="inline-flex min-h-10 items-center gap-1.5 rounded-md border border-line px-2.5 py-1 text-sm font-semibold text-ink hover:border-line-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink"
						onclick={() => theme.toggle()}
						aria-label={theme.resolved === 'dark' ? m.status_public_switch_light() : m.status_public_switch_dark()}
					>
						{#if theme.resolved === 'dark'}<Sun class="size-4" aria-hidden="true" />{m.status_public_light()}{:else}<Moon class="size-4" aria-hidden="true" />{m.status_public_dark()}{/if}
					</button>
				{/if}
			</header>

			<!-- Overall banner: the one-second answer. -->
			<section
				class={`rise-in mt-6 flex flex-wrap items-center justify-between gap-3 rounded-[var(--radius-card)] border px-4 py-4 shadow-lift sm:px-5 ${bannerTone === 'signal' ? 'border-signal/30 bg-signal-soft' : bannerTone === 'advisory' ? 'border-advisory/35 bg-advisory-soft' : bannerTone === 'warning' ? 'border-warning/35 bg-warning-soft' : 'border-info/30 bg-info-soft'}`}
				style="--rise-delay: 40ms"
				aria-live="polite"
				aria-label={m.status_public_current_status()}
			>
				<div class="flex min-w-0 items-center gap-3">
					<Plate tone={bannerTone} size="md" label={banner.plate} />
					<p class="display text-xl text-ink sm:text-2xl">
						{#if simple}{banner.label}{:else}<DecryptText text={banner.label} tag="span" />{/if}
					</p>
				</div>
				{#if lastChecked}
					<p class="text-[0.8125rem] text-ink-2">
						{m.status_public_checked()} <time class="tnum" datetime={lastChecked.toISOString()} title={formatDateTime(lastChecked)}>{formatRelative(lastChecked)}</time>
					</p>
				{/if}
			</section>

			{#if !simple}
				<StatusMascot tone={bannerTone} />
			{/if}

			{@render announcements()}
			{@render services()}
			{@render pastIncidents()}
			<StatusUpdates slug={status.page.slug} kinds={status.page.subscribe_kinds ?? []} services={subscribeServices} />
		{/if}

		{@render pageFooter()}
	</main>
</div>
