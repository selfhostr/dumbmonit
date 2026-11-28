<script lang="ts">
	import { kindLabel as kindLabelOf } from '#lib/kind-label.js';
	import { m } from '#lib/paraglide/messages.js';
	/**
	 * Device detail: one large faceplate band, the alert timeline of this device, then
	 * the instruments — availability for a service; for a device, the essentials
	 * open and everything else folded (containers, interfaces, the long tail),
	 * each chart mounted only once its section is opened. "Probe now" is the
	 * main diagnostic tool: it answers "why is this device silent?" without
	 * leaving the page.
	 */
	import { tick, untrack } from 'svelte';
	import { page } from '$app/state';
	import { goto } from '#lib/navigation.js';
	import {
		ApiError,
		deleteTarget,
		getTarget,
		listAlertHistory,
		listAlertRules,
		listAlerts,
		listCollectors,
		probeTarget,
		queryInstant,
		queryRange,
		updateTarget,
		type Alert,
		type AlertRule,
		type AlertHistoryEntry,
		type CollectorInfo,
		type ProbeReport,
		type Target,
		type TargetPayload
	} from '#lib/api/index.js';
	import {
		displayState,
		isUptimeKind,
		STATE_TONE,
		type ProbeStatus
	} from '#lib/format.js';
	import {
		loadProbeStatuses,
		loadResponseTimesWindow,
		loadHttpPhases,
		loadResponseTimes,
		loadUptimeHistory,
		loadUptimeSummary,
		loadScenarioSamples,
		RANGES,
		type HistorySlot,
		type RangeId,
		type ScenarioSamples,
		type UptimeSummary
	} from '#lib/metrics.js';
	import { lastRun, parseSteps } from '#lib/components/device-form/scenario-steps.js';
	import type { ChartEvent, Serie } from '#lib/components/Chart.svelte';
	import AnnotationsPanel from '#lib/components/annotations/AnnotationsPanel.svelte';
	import { eventsFromHistory, eventsFromRestarts, thresholdsFor } from '#lib/chart-context.js';
	import { CONTAINER_RESTART_METRIC, UPTIME_METRIC, restartEvents, type RestartEvent } from '#lib/restart-events.js';
	import Segmented from '#lib/components/devices/Segmented.svelte';
	import ProbeLocationsCard from '#lib/components/devices/ProbeLocationsCard.svelte';
	import DeviceTimeline from '#lib/components/devices/DeviceTimeline.svelte';
	import { countAlertsByTarget } from '#lib/alerts-attention.js';
	import CveDeviceCard from '#lib/components/cves/CveDeviceCard.svelte';
	import MachineHealthCard from '#lib/components/devices/agent/MachineHealthCard.svelte';
	import TargetRemediation from '#lib/components/remediation/TargetRemediation.svelte';
	import SecurityReport from '#lib/components/devices/SecurityReport.svelte';
	import { getTargetSecurity } from '#lib/api/security.js';
	import type { SecurityReport as SecurityReportData } from '#lib/api/index.js';
	import { hasKindPanel } from '#lib/components/devices/kinds.js';
	import {
		loadDeviceMetrics,
		loadDeviceMetricsWindow,
		sectionize,
		type DeviceMetricGroup
	} from '#lib/components/devices/metrics.js';
	import { catalogLayout, ratedFamilies, withCatalogUnits } from '#lib/components/devices/catalog.js';
	import MaintenanceWindows from '#lib/components/devices/MaintenanceWindows.svelte';
	import { neighbours } from '#lib/components/devices/neighbours.js';
	import { buildRack } from '#lib/components/devices/rack.js';
	import { buildFolders } from '#lib/components/devices/folders.js';
	import { splitDisabled, freeTags } from '#lib/components/devices/listState.js';
	import { alertsStore } from '#lib/stores/alerts.svelte.js';
	import { startPolling } from '#lib/poll.js';
	import { tagChip } from '#lib/tag-chip.js';
	import { dismissals } from '#lib/stores/dismissals.svelte.js';
	import DockerStrip from '#lib/components/devices/docker/DockerStrip.svelte';
	import { isDockerDevice } from '#lib/components/devices/agent/services.js';
	import TargetDeadlines from '#lib/components/deadlines/TargetDeadlines.svelte';
	import TrafficSection from '#lib/components/flows/TrafficSection.svelte';
	import {
		Button,
		EmptyState,
		ErrorNotice,
		Skeleton,
		confetti,
		type Tone
	} from '#lib/ui/index.js';
	import { PowerOff, ServerOff } from 'lucide-svelte';
	import DetailNav from '#lib/components/devices/detail/DetailNav.svelte';
	import DeviceFolds from '#lib/components/devices/detail/DeviceFolds.svelte';
	import HeaderActions from '#lib/components/devices/detail/HeaderActions.svelte';
	import HeaderIdentity from '#lib/components/devices/detail/HeaderIdentity.svelte';
	import HeaderNotices from '#lib/components/devices/detail/HeaderNotices.svelte';
	import KindPanelSection from '#lib/components/devices/detail/KindPanelSection.svelte';
	import ServiceInstruments from '#lib/components/devices/detail/ServiceInstruments.svelte';

	const id = $derived(Number(page.params.id));

	// --- Target ---------------------------------------------------------------

	let target = $state<Target | null>(null);
	let loading = $state(true);

	// The very first device of the instance: a burst of confetti, once, as its
	// page opens (the add form says so in the navigation state).
	let celebrated = false;
	$effect(() => {
		if (celebrated || !target) return;
		if (!(page.state as { firstDevice?: boolean }).firstDevice) return;
		celebrated = true;
		confetti();
	});
	let error = $state<unknown>(null);
	let parent = $state<Target | null>(null);
	/** The relay agent that probes this device, when it is not the server. */
	let relay = $state<Target | null>(null);
	let collectors = $state<CollectorInfo[]>([]);
	let deviceProbe = $state<ProbeStatus | undefined>(undefined);

	const missing = $derived(error instanceof ApiError && error.status === 404);
	const service = $derived(target !== null && isUptimeKind(target.kind));
	const collector = $derived(collectors.find((c) => c.kind === target?.kind) ?? null);
	const kindLabel = $derived(target ? kindLabelOf(target.kind, collector?.label) : '');

	/**
	 * An agent's address is its opaque host id; the machine's own name and OS
	 * come from the `dumbmonit_agent_os_info` labels, read once per device.
	 */
	let osInfo = $state<Record<string, string> | null>(null);
	const hostname = $derived(osInfo?.host || osInfo?.hostname || null);
	/**
	 * What the line shows after the type: the machine's name for an agent (its
	 * address is an opaque id), the address for anything else.
	 */
	const identity = $derived.by(() => {
		if (!target) return '';
		return target.kind === 'agent' ? (hostname ?? '') : target.address;
	});

	// --- Free tags, and the monitors around this one in the list ------------
	/** Free tags (not the options of the type) as chips; the options live in the form. */
	const tagChips = $derived.by(() => {
		if (!target) return [];
		const keys = new Set((collector?.options ?? []).map((o) => o.key));
		return freeTags(target.tags, keys)
			.map((pair) => {
				const at = pair.indexOf('=');
				return tagChip(pair.slice(0, at), pair.slice(at + 1));
			})
			.sort((a, b) => a.label.localeCompare(b.label, 'en'));
	});

	/** The list's default order (folders, state tier, manual position, name), disabled monitors last. */
	const listOrder = $derived.by(() => {
		const all = alertsStore.targets;
		if (all.length === 0) return [];
		const stateOf = (t: Target) => displayState(t, alertsStore.probes.get(t.id));
		const { active, disabled } = splitDisabled(all, stateOf);
		return [...buildFolders(all, stateOf, active).flatMap((section) => section.rows), ...buildRack(all, stateOf, disabled)].map(
			(row) => row.target
		);
	});
	const around = $derived.by(() => {
		const at = listOrder.findIndex((t) => t.id === id);
		return at === -1 ? { prev: null, next: null } : neighbours(listOrder, listOrder[at]);
	});

	// --- Security score (opt-in in Settings) ---------------------------------
	// A badge in the header when the score is on and this kind is rated; the
	// report opens under the header, straight away when the link said so
	// (`#security`, from the badge in the device list). A failed read only
	// hides the badge: the score is secondary, the page has its own errors.

	let security = $state<SecurityReportData | null>(null);
	let securityOpen = $state(false);
	// No grade (nothing to rate yet): no badge, rather than a grey one.
	const securityShown = $derived(security?.enabled && security.supported && security.grade !== null ? security : null);

	$effect(() => {
		const targetId = id;
		security = null;
		securityOpen = untrack(() => page.url.hash === '#security');
		return startPolling(
			(signal) =>
				getTargetSecurity(targetId, signal)
					.then((report) => {
						security = report;
						return true;
					})
					.catch(() => false),
			60_000
		);
	});

	/** Scrolls the report into view once it is there (opened from a link or the badge). */
	let securityScrolled = false;
	$effect(() => {
		if (!securityOpen || !securityShown) {
			securityScrolled = false;
			return;
		}
		if (securityScrolled) return;
		securityScrolled = true;
		void tick().then(() => document.getElementById('security-report')?.scrollIntoView({ block: 'nearest' }));
	});

	// --- Range (kept in the URL so a link carries the view) -----------------

	function validRange(raw: string | null): RangeId {
		return RANGES.some((r) => r.id === raw) ? (raw as RangeId) : '1h';
	}
	const range = $derived(validRange(page.url.searchParams.get('range')));
	const rangeSeconds = $derived(RANGES.find((r) => r.id === range)?.seconds ?? 3600);
	const rangeLabel = $derived(RANGES.find((r) => r.id === range)?.label ?? m.devices_range_1h());
	const rangeOptions = RANGES.map((r) => ({ id: r.id, label: r.id }));

	function setRange(next: RangeId) {
		if (next === range) return;
		void goto(`/targets/${id}?range=${next}`, { replace: true, reset: false });
	}

	// --- Device metrics -------------------------------------------------------

	let groups = $state<DeviceMetricGroup[]>([]);
	let loadingMetrics = $state(true);
	let metricsError = $state<unknown>(null);

	/** Counters the kind's catalogue charts: asked as rates, reloaded once the catalogue arrives. */
	const rated = $derived(ratedFamilies(collector));
	const ratedKey = $derived(rated.join(','));
	/** Families with the catalogue's units and labels; its views, when the kind declares some. */
	const shown = $derived(withCatalogUnits(groups, collector));
	const sections = $derived(sectionize(shown));
	const layout = $derived(catalogLayout(shown, collector));
	/** The fold under the charts: everything else, or every family when the catalogue picked the charts. */
	const tail = $derived(layout ? sections.rest : sections.other);

	/** Interface rows unfolded by the user; the search over the long tail. */
	/** Interface rows unfolded by the user; the search over the long tail; flat families folded. */
	let openInterfaces = $state<Record<string, boolean>>({});
	let otherQuery = $state('');
	let showSteady = $state(false);

	// --- Service instruments --------------------------------------------------

	let summary = $state<UptimeSummary | null>(null);
	let history = $state<HistorySlot[]>([]);
	let responseTimes = $state<Serie[]>([]);
	let phaseTimes = $state<Serie[]>([]);
	/** Per-step series of an HTTP scenario; `null` for any other kind. */
	let scenarioSamples = $state<ScenarioSamples | null>(null);
	const scenarioRuns = $derived(
		scenarioSamples && target
			? lastRun(scenarioSamples.results, scenarioSamples.statuses, scenarioSamples.durations, parseSteps(target.tags.steps))
			: []
	);
	let checks = $state<number | null>(null);
	let loadingUptime = $state(true);
	let uptimeError = $state<unknown>(null);

	/** For a service the truth is the last probe result; for a device, the last poll. */
	const stateNow = $derived(
		target ? displayState(target, service ? (summary?.status ?? deviceProbe) : deviceProbe) : 'unknown'
	);
	const tone = $derived(STATE_TONE[stateNow]);
	const blink = $derived(stateNow === 'offline' || stateNow === 'down');

	/** Availability over the closest window the summary offers. */
	const availability = $derived(
		summary ? (range === '7d' ? summary.availability.week : summary.availability.day) : null
	);
	const availabilityWindow = $derived(range === '7d' ? m.devices_target_window_7d() : m.devices_target_window_24h());
	/** Certificate plate: expired is a warning, under two weeks an advisory. */
	const certificate = $derived.by(() => {
		const days = summary?.certExpiryDays ?? null;
		if (days === null) return null;
		const whole = Math.round(days);
		if (whole < 0) return { tone: 'warning' as Tone, label: m.devices_target_cert_expired({ days: -whole }) };
		return {
			tone: (whole < 14 ? 'advisory' : 'signal') as Tone,
			label: m.devices_target_cert_expires({ days: whole })
		};
	});

	// --- Alerts on this device -----------------------------------------------

	let alerts = $state<Alert[]>([]);

	// Context laid over this page's charts: rule thresholds and alert transitions.
	let chartRules = $state.raw<AlertRule[]>([]);
	let chartHistory = $state.raw<AlertHistoryEntry[]>([]);
	let chartRestarts = $state.raw<RestartEvent[]>([]);
	/** Change markers (deployments, updates) of this device, from the panel above the charts. */
	let annotationEvents = $state.raw<ChartEvent[]>([]);
	const chartEvents = $derived(
		[...eventsFromHistory(chartHistory, chartRules, id), ...eventsFromRestarts(chartRestarts), ...annotationEvents].sort((a, b) => a.at - b.at)
	);
	/** One cursor for every chart of this device page. */
	const syncKey = $derived(`target-${id}`);
	const thresholdsOf = (metric: string, unit: string, scale = 1) =>
		thresholdsFor(chartRules, id, metric, unit, scale);
	/** One query serves every chart that asks for last week's window (5 min buckets). */
	let previousCache: { key: string; groups: Promise<DeviceMetricGroup[]> } | null = null;
	const previousOf = (metric: string) => async (window: { start: number; end: number }) => {
		const key = `${id}|${Math.floor(window.start / 300)}|${Math.floor(window.end / 300)}`;
		if (previousCache?.key !== key) {
			const groups = loadDeviceMetricsWindow(id, window, undefined, rated);
			groups.catch(() => {
				if (previousCache?.groups === groups) previousCache = null;
			});
			previousCache = { key, groups };
		}
		return (await previousCache.groups).find((g) => g.name === metric)?.series ?? [];
	};
	const previousResponse = (window: { start: number; end: number }) => loadResponseTimesWindow(id, window);

	$effect(() => {
		const targetId = id;
		const seconds = rangeSeconds;
		const controller = new AbortController();
		const since = new Date(Date.now() - seconds * 1000).toISOString();
		void Promise.all([
			listAlertRules(controller.signal).catch(() => null),
			listAlertHistory({ since, limit: 500 }, controller.signal).catch(() => null)
		]).then(([rules, history]) => {
			if (controller.signal.aborted || targetId !== id) return;
			if (rules) chartRules = rules;
			if (history) chartHistory = history;
		});
		return () => controller.abort();
	});
	// Restart markers: agent reboots and Docker container restarts, read from the stored series.
	$effect(() => {
		const targetId = id;
		const seconds = rangeSeconds;
		const controller = new AbortController();
		const end = Date.now() / 1000;
		void queryRange(
			{
				query: `{__name__=~"${UPTIME_METRIC}|${CONTAINER_RESTART_METRIC}", target="${targetId}"}`,
				start: end - seconds,
				end,
				// A restart must show between two samples: never coarser than a few minutes.
				step: Math.min(300, Math.max(30, Math.round(seconds / 600)))
			},
			controller.signal
		)
			.then((found) => {
				if (controller.signal.aborted || targetId !== id) return;
				chartRestarts = restartEvents(found);
			})
			.catch(() => {
				// Decoration only: the charts stand without it.
			});
		return () => controller.abort();
	});
	/** Bell in the header: this monitor's active alerts, counted like the list does. */
	const headerAlertCounts = $derived(countAlertsByTarget(alerts).get(id));
	/** Bumped on each 30 s refresh so the timeline and the silence plate follow. */
	let refreshKey = $state(0);
	// A dismissal or its undo (from the toast too) moves this monitor's alerts.
	let seenDismissals = dismissals.version;
	$effect(() => {
		const version = dismissals.version;
		if (version === seenDismissals) return;
		seenDismissals = version;
		untrack(() => {
			void loadContext();
			refreshKey += 1;
		});
	});
	/** The alert story has nothing to tell (never fired). */
	let timelineEmpty = $state(false);

	// --- Actions --------------------------------------------------------------

	let probing = $state(false);
	let probeResult = $state<ProbeReport | null>(null);
	let probeError = $state<unknown>(null);
	let probeTimer: ReturnType<typeof setTimeout> | null = null;

	/** False once the monitor is switched off: nothing to probe, nothing collected. */
	const monitorOn = $derived(target?.enabled ?? true);
	/** Switched off: nothing is collected, so the live panels (and a measurements block with nothing in it) give way to one notice. */
	const idle = $derived(target !== null && !target.enabled);
	const noMeasurements = $derived(
		service
			? !loadingUptime && responseTimes.length === 0 && availability === null
			: !loadingMetrics && groups.length === 0
	);
	let savingEnabled = $state(false);
	let enabledError = $state<unknown>(null);

	let deleting = $state(false);
	/** An agent's monitor asks first whether to take the agent off the machine. */
	let askAgentRemoval = $state(false);
	let deleteError = $state<unknown>(null);

	/** Everything the server needs to keep the target as is. The credential is omitted: the server keeps it. */
	function payloadOf(t: Target, enabled: boolean): TargetPayload {
		return {
			name: t.name,
			address: t.address,
			kind: t.kind,
			profile_id: t.profile_id,
			parent_id: t.parent_id,
			interval_secs: t.interval_secs,
			enabled,
			tags: t.tags
		};
	}

	async function setEnabled(value: boolean) {
		if (!target) return;
		savingEnabled = true;
		enabledError = null;
		try {
			target = await updateTarget(target.id, payloadOf(target, value));
		} catch (cause) {
			enabledError = cause;
		} finally {
			savingEnabled = false;
		}
	}

	async function probe() {
		probing = true;
		probeError = null;
		probeResult = null;
		if (probeTimer) clearTimeout(probeTimer);
		try {
			probeResult = await probeTarget(id);
			// A successful probe changes the state and adds points: refresh both
			// so the user sees the effect right away.
			await Promise.all([loadTarget(), loadInstruments()]);
			probeTimer = setTimeout(() => (probeResult = null), 8000);
		} catch (cause) {
			probeError = cause;
		} finally {
			probing = false;
		}
	}

	async function remove() {
		deleting = true;
		deleteError = null;
		try {
			await deleteTarget(id);
			await goto('/targets');
		} catch (cause) {
			deleteError = cause;
			deleting = false;
		}
	}

	// --- Loading --------------------------------------------------------------

	async function loadTarget(signal?: AbortSignal) {
		error = null;
		try {
			const next = await getTarget(id, signal);
			target = next;
			if (next.parent_id !== null && parent?.id !== next.parent_id) {
				parent = await getTarget(next.parent_id, signal).catch(() => null);
			} else if (next.parent_id === null) {
				parent = null;
			}
			if (next.via_agent !== null && relay?.id !== next.via_agent) {
				relay = await getTarget(next.via_agent, signal).catch(() => null);
			} else if (next.via_agent === null) {
				relay = null;
			}
		} catch (cause) {
			if (cause instanceof DOMException && cause.name === 'AbortError') return;
			error = cause;
			return false;
		} finally {
			loading = false;
		}
	}

	/** Side data that must never block the page: kind labels, device probe state, alerts. */
	async function loadContext(signal?: AbortSignal) {
		const [kinds, probes, active] = await Promise.all([
			listCollectors(signal).catch(() => null),
			loadProbeStatuses(signal).catch(() => null),
			listAlerts(signal).catch(() => null)
		]);
		if (kinds) collectors = kinds;
		if (probes) deviceProbe = probes.get(id);
		if (active) {
			alerts = active.filter(
				(a) => a.target_id === id && ['firing', 'pending', 'suppressed'].includes(a.effective_phase)
			);
		}
	}

	async function loadOsInfo(signal?: AbortSignal) {
		try {
			const series = await queryInstant(`last_over_time(dumbmonit_agent_os_info{target="${id}"}[1d])`, signal);
			osInfo = series[0]?.metric ?? null;
		} catch {
			osInfo = null;
		}
	}

	async function loadMetrics(signal?: AbortSignal) {
		metricsError = null;
		try {
			groups = await loadDeviceMetrics(id, rangeSeconds, signal, rated);
		} catch (cause) {
			if (cause instanceof DOMException && cause.name === 'AbortError') return;
			metricsError = cause;
		} finally {
			loadingMetrics = false;
		}
	}

	async function loadUptime(signal?: AbortSignal) {
		uptimeError = null;
		try {
			const scenario = target?.kind === 'http_steps';
			const [s, h, r, c, p, steps] = await Promise.all([
				loadUptimeSummary(id, signal),
				loadUptimeHistory(id, rangeSeconds, signal),
				loadResponseTimes(id, rangeSeconds, signal),
				queryInstant(`count_over_time(dumbmonit_probe_success{target="${id}"}[${range}])`, signal),
				// The breakdown is a bonus: its failure must not hide the rest.
				loadHttpPhases(id, rangeSeconds, signal).catch((cause) => {
					if (cause instanceof DOMException && cause.name === 'AbortError') throw cause;
					return [] as Serie[];
				}),
				// Same for the steps of a scenario.
				scenario
					? loadScenarioSamples(id, signal).catch((cause) => {
							if (cause instanceof DOMException && cause.name === 'AbortError') throw cause;
							return null;
						})
					: Promise.resolve(null)
			]);
			summary = s;
			history = h;
			responseTimes = r;
			phaseTimes = p;
			scenarioSamples = steps;
			const count = Number(c[0]?.values?.[0]?.[1]);
			checks = Number.isFinite(count) ? count : 0;
		} catch (cause) {
			if (cause instanceof DOMException && cause.name === 'AbortError') return;
			uptimeError = cause;
		} finally {
			loadingUptime = false;
		}
	}

	function loadInstruments(signal?: AbortSignal) {
		return service ? loadUptime(signal) : loadMetrics(signal);
	}

	// The id changes when navigating between devices without leaving the page.
	$effect(() => {
		void id;
		loading = true;
		target = null;
		groups = [];
		summary = null;
		history = [];
		responseTimes = [];
		phaseTimes = [];
		scenarioSamples = null;
		alerts = [];
		osInfo = null;
		probeResult = null;
		probeError = null;
		// The instruments load in their own effect at first, then here with the
		// rest; only the device itself decides whether the page backs off.
		let first = true;
		return startPolling(async (signal) => {
			const reads: Promise<unknown>[] = [loadTarget(signal), loadContext(signal)];
			if (!first) {
				reads.push(loadInstruments(signal));
				refreshKey += 1;
			}
			first = false;
			const [targetOk] = await Promise.all(reads);
			return targetOk !== false;
		}, 30_000);
	});

	// Instruments need the kind (service or device) and follow the range.
	$effect(() => {
		void rangeSeconds;
		void ratedKey;
		const kind = target?.kind;
		if (!kind) return;
		const controller = new AbortController();
		// The skeleton only shows before the first data; the reads are untracked
		// so that filling the instruments does not re-trigger their own load.
		untrack(() => {
			if (isUptimeKind(kind)) {
				loadingUptime = summary === null;
				void loadUptime(controller.signal);
			} else {
				loadingMetrics = groups.length === 0;
				void loadMetrics(controller.signal);
				if (kind === 'agent' && osInfo === null) void loadOsInfo(controller.signal);
			}
		});
		return () => controller.abort();
	});

	$effect(() => () => {
		if (probeTimer) clearTimeout(probeTimer);
	});
</script>

<svelte:head><title>{m.devices_target_page_title({ name: target?.name ?? m.devices_target_title_fallback() })}</title></svelte:head>

<DetailNav {around} />

{#if missing}
	<EmptyState icon={ServerOff} title={m.devices_target_gone_title()} description={m.devices_target_gone_desc()}>
		{#snippet action()}
			<Button variant="secondary" href="/targets">{m.devices_target_back_btn()}</Button>
		{/snippet}
	</EmptyState>
{:else if error}
	<ErrorNotice {error} title={m.devices_target_err_load()} onretry={() => void loadTarget()} />
{:else if loading || !target}
	<div class="flex flex-col gap-6" aria-busy="true" aria-label={m.devices_target_loading()}>
		<Skeleton class="h-40 w-full rounded-[var(--radius-card)]" />
		<Skeleton class="h-10 w-64" />
		<div class="grid gap-4 lg:grid-cols-2">
			<Skeleton class="h-72 w-full rounded-[var(--radius-card)]" rows={2} />
		</div>
	</div>
{:else}
	<!-- Header band: flat surface, identity on the left, one main action on the right. -->
	<section class="rise-in overflow-x-clip rounded-[var(--radius-card)] border border-line bg-surface px-4 py-4 shadow-lift sm:px-6 sm:py-5" aria-labelledby="device-name">
		<div class="flex flex-col gap-5 max-sm:gap-3 lg:flex-row lg:items-start lg:justify-between">
			<HeaderIdentity {target} {id} {parent} {relay} {tone} {blink} {stateNow} {kindLabel} {identity} {tagChips} {certificate} {summary} {securityShown} bind:securityOpen {headerAlertCounts} />

			<HeaderActions {target} {refreshKey} {probing} {savingEnabled} {deleting} {probe} {setEnabled} {remove} askRemoval={() => (askAgentRemoval = true)} />
		</div>

		<HeaderNotices {target} {collector} {kindLabel} {probeResult} {probeError} {enabledError} {deleteError} {deleting} bind:askAgentRemoval {probe} {remove} {loadTarget} />
	</section>

	{#if target.note}
		<section class="mt-4 rounded-[var(--radius-card)] border border-line bg-surface px-4 py-3 sm:px-6" aria-labelledby="monitor-note-title">
			<h2 id="monitor-note-title" class="label-tape">{m.w1monitors_detail_note_title()}</h2>
			<p class="mt-1 text-sm break-words whitespace-pre-wrap text-ink">{target.note}</p>
		</section>
	{/if}

	<MaintenanceWindows {target} {refreshKey} class="mt-4" />

	{#if idle}
		<!-- One line instead of a stack of empty panels. -->
		<div class="mt-6 flex items-center gap-3 rounded-[var(--radius-card)] border border-dashed border-line bg-surface px-4 py-3" role="status">
			<PowerOff class="size-5 shrink-0 text-ink-3" aria-hidden="true" />
			<p class="min-w-0 text-sm text-ink-2">{m.devices_target_disabled_empty()}</p>
		</div>
	{/if}

	{#if securityShown && securityOpen}
		<SecurityReport report={securityShown} onclose={() => (securityOpen = false)} />
	{/if}

	{#if target.multisite && !idle}
		<ProbeLocationsCard targetId={target.id} {refreshKey} />
	{/if}

	<!-- What makes a machine slow, at a glance (agent hosts only) -->
	{#if target.kind === 'agent' && !idle}
		{#key target.id}
			<MachineHealthCard {target} {refreshKey} />
		{/key}
	{/if}

	<CveDeviceCard targetId={target.id} />

	<!-- Expiry dates this monitor reports (certificates, domain, licences…); absent when it has none. -->
	<TargetDeadlines target={target.id} class="mt-6" />

	<!-- Top talkers of a flow exporter (NetFlow, IPFIX, sFlow); absent when it sends none. -->
	{#key target.id}
		<TrafficSection target={target.id} class="mt-6" />
	{/key}

	<!-- Containers at a glance, with the fleet-wide policies; the list itself sits with the metrics.
	     Docker is a device of its own under the machine whose agent found it. -->
	{#if isDockerDevice(target) && !idle}
		<DockerStrip {target} />
	{/if}

	<!-- The story of this device: what fires now, what fired before -->
	<!-- Nothing ever fired: no section at all, the header already says how it is doing. -->
	<section class={`mt-6 ${timelineEmpty ? 'hidden' : ''}`} aria-labelledby="device-alerts">
		<h2 id="device-alerts" class="mb-3 text-base font-semibold tracking-tight text-ink">{m.devices_target_alerts_heading()}</h2>
		<DeviceTimeline
			targetId={id}
			target={target ?? undefined}
			{alerts}
			{refreshKey}
			bind:empty={timelineEmpty}
			onackchange={() => void loadContext()}
		/>
	</section>

	<!-- Runbooks that apply to this device, and its recent remediation runs -->
	{#key target.id}
		<TargetRemediation targetId={target.id} />
	{/key}

	<!-- What this kind of device has to show beyond charts (guests, backup calendar, disks…) -->
	{#if hasKindPanel(target.kind) && !idle}
		<KindPanelSection {target} />
	{/if}

	<!-- Instruments: a website-change watcher already says everything in its own panel -->
	{#if target.kind !== 'webchange' && !(idle && noMeasurements)}
	<section class="mt-8" aria-labelledby="device-metrics">
		<div class="mb-4 flex min-w-0 flex-wrap items-center justify-between gap-3">
			<h2 id="device-metrics" class="text-base font-semibold tracking-tight text-ink">
				{service ? m.devices_target_availability() : m.devices_target_metrics()}
				<!-- A service's figures each name their own window (24 h, last hour, the range):
				     the heading does not claim one for all of them. -->
				{#if !service}<span class="ml-1 font-normal text-ink-2">{m.devices_target_last_range({ range: rangeLabel })}</span>{/if}
			</h2>
			<Segmented options={rangeOptions} value={range} onchange={setRange} label={m.devices_target_time_range()} size="sm" />
		</div>

		<AnnotationsPanel targetId={id} seconds={rangeSeconds} bind:events={annotationEvents} />

		{#if service}
			{#if uptimeError}
				<ErrorNotice error={uptimeError} title={m.devices_target_err_availability()} onretry={() => void loadUptime()} />
			{:else if loadingUptime}
				<div class="grid gap-4" aria-busy="true">
					<Skeleton class="h-20 w-full" />
					<Skeleton class="h-16 w-full rounded-[var(--radius-card)]" />
					<Skeleton class="h-64 w-full rounded-[var(--radius-card)]" />
				</div>
			{:else}
				<ServiceInstruments {target} {summary} {availability} {availabilityWindow} {rangeLabel} {checks} {history} {responseTimes} {phaseTimes} {scenarioRuns} {syncKey} {chartEvents} {thresholdsOf} {previousResponse} {monitorOn} {probing} {probe} />
			{/if}
		{:else if metricsError}
			<ErrorNotice error={metricsError} title={m.devices_target_err_metrics()} onretry={() => void loadMetrics()} />
		{:else if loadingMetrics}
			<div class="grid gap-4 lg:grid-cols-2" aria-busy="true">
				<Skeleton class="h-72 w-full rounded-[var(--radius-card)]" rows={2} />
			</div>
		{:else if groups.length === 0 && target.kind !== 'agent'}
			<EmptyState title={m.devices_target_no_data_title()} description={target.enabled ? m.devices_target_no_data_desc() : m.devices_target_disabled_empty()}>
				{#snippet action()}
					{#if monitorOn}
						<Button variant="secondary" onclick={probe} loading={probing}>{m.devices_target_probe_now()}</Button>
					{/if}
				{/snippet}
			</EmptyState>
		{:else}
			<DeviceFolds {target} {layout} {sections} {tail} {groups} {loadingMetrics} {monitorOn} {probing} {probe} {syncKey} {chartEvents} {thresholdsOf} {previousOf} bind:openInterfaces bind:otherQuery bind:showSteady />
		{/if}
	</section>
{/if}
{/if}
