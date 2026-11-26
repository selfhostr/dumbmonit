<script lang="ts">
	/**
	 * Wall mode: a room monitor that is also a picture. The whole screen is a
	 * world (Paris by default: the Eiffel Tower, the rooftops, the pigeons)
	 * under a sky that follows the real hour, with the weather of the network:
	 * clear when all is well, clouds for advisories, rain when something is
	 * down, a thunderstorm in a real outage. On top of it, readable across the
	 * room: the verdict in a few huge words ("All good" / "N problems"), the
	 * clock and the date, the problems as tiles, then every monitor as a small
	 * tile. The fleet figures lie in a quiet strip along the foot,
	 * on the stone of the rooftop.
	 *
	 * Reads the same stores as the console (monitors, probes and alerts every
	 * 30 s; the figures from the fleet store). Escape or Exit
	 * goes back to the console. The screen stays awake while the wall is open.
	 *
	 * The world (Settings sets the default, `?city=` overrides it, the Backdrop
	 * button changes it for this browser) is loaded on demand: a plain board
	 * (`none`) loads nothing. The wall is always Night; on a phone or a hidden
	 * tab the world holds still, and under `prefers-reduced-motion` it is a
	 * still picture. Beside the date, the outside temperature and an icon of
	 * the weather when Settings gives the wall a place.
	 */
	import { untrack, type Component } from 'svelte';
	import { goto } from '#lib/navigation.js';
	import { page } from '$app/state';
	import { AlertTriangle, CircleAlert, CircleCheck, CircleDashed, Maximize, Minimize, Volume2, VolumeX, X } from 'lucide-svelte';
	import { listAlertRules, type AlertRule } from '#lib/api/index.js';
	import { getWallWeather } from '#lib/api/weather.js';
	import type { WallWeather as OutdoorWeather } from '#lib/api/types.js';
	import { ApiError } from '#lib/api/client.js';
	import { countAlertsByTarget, criticalCount, quietedOnWall, visibleOnWall } from '#lib/alerts-attention.js';
	import { readSky } from '#lib/components/overview/sky.js';
	import { fleetStore } from '#lib/components/enterprise/fleet.svelte.js';
	import { mean } from '#lib/components/enterprise/fleet.js';
	import { consoleStatus } from '#lib/components/enterprise/status.js';
	import { formatRelative } from '#lib/format.js';
	import { m } from '#lib/paraglide/messages.js';
	import { getLocale } from '#lib/paraglide/runtime.js';
	import { alertsStore } from '#lib/stores/alerts.svelte.js';
	import { auth } from '#lib/stores/auth.svelte.js';
	import { instanceStore } from '#lib/stores/instance.svelte.js';
	import { viewport } from '#lib/stores/viewport.svelte.js';
	import { useSceneLive } from '#lib/scene/live.svelte.js';
	import { Button, Skeleton, calmMotion } from '#lib/ui/index.js';
	import WallBand, { type BandFigure } from '#lib/components/wall/WallBand.svelte';
	import WallOutdoor from '#lib/components/wall/WallOutdoor.svelte';
	import WallSignOut from '#lib/components/wall/WallSignOut.svelte';
	import { OUTDOOR_REFRESH_MS, OUTDOOR_RETRY_MS } from '#lib/components/wall/outdoor.js';
	import WallDevices from '#lib/components/wall/WallDevices.svelte';
	import WallProblems from '#lib/components/wall/WallProblems.svelte';
	import WallSceneControl from '#lib/components/wall/WallSceneControl.svelte';
	import WallNotice from '#lib/components/wall/WallNotice.svelte';
	import { clockText, parseWallParams, readWallPrefs, writeWallPrefs, type WallPrefs } from '#lib/components/wall/wallPrefs.js';
	import { beep, unlockWallSound } from '#lib/components/wall/wallSound.js';
	import {
		DEFAULT_WALL_SCENE,
		parseForcedWallScene,
		readWallScene,
		resolveWallScene,
		writeWallScene,
		type WallSceneChoice
	} from '#lib/components/wall/wallScene.js';
	import { parseForcedWeather, weatherFor, weatherForTone, type WallWeather } from '#lib/components/wall/wallWeather.js';

	let now = $state(new Date());
	let rules = $state<AlertRule[]>([]);

	// The shared stores poll from the layout; this view only asks the fleet
	// store to keep its figures fresh, and reads the rules once.
	$effect(() => untrack(() => fleetStore.start()));
	$effect(() => {
		const controller = new AbortController();
		listAlertRules(controller.signal)
			.then((list) => (rules = list))
			.catch(() => {});
		return () => controller.abort();
	});

	// The weather outside, beside the date. The server asks Open-Meteo (browsers
	// never do) and keeps the reading ten minutes; a failure, or no place chosen
	// in Settings, leaves the wall without it, silently, and the wall asks again
	// after a minute rather than ten, so a city just chosen shows up quickly.
	let outdoor = $state<OutdoorWeather | null>(null);
	$effect(() => {
		const controller = new AbortController();
		let timer: ReturnType<typeof setTimeout> | undefined;
		let stopped = false;
		const poll = async () => {
			if (document.visibilityState === 'visible') {
				try {
					outdoor = await getWallWeather(controller.signal);
				} catch (cause) {
					if (cause instanceof DOMException && cause.name === 'AbortError') return;
					// Not shown: keep the last reading, if any, until the next try.
					// A server without the route will not have one: stop asking.
					if (cause instanceof ApiError && cause.missing) stopped = true;
				}
			}
			if (!stopped && !controller.signal.aborted) timer = setTimeout(poll, outdoor?.current ? OUTDOOR_REFRESH_MS : OUTDOOR_RETRY_MS);
		};
		untrack(() => void poll());
		return () => {
			controller.abort();
			clearTimeout(timer);
		};
	});

	// One tick per second drives the clock and "updated … ago".
	$effect(() => {
		const timer = setInterval(() => (now = new Date()), 1000);
		return () => clearInterval(timer);
	});

	// Keep the screen on; browsers release the lock when the tab is hidden.
	$effect(() => {
		let lock: WakeLockSentinel | null = null;
		let disposed = false;
		const request = async () => {
			if (disposed || document.visibilityState !== 'visible') return;
			try {
				lock = (await navigator.wakeLock?.request('screen')) ?? null;
			} catch {
				// Not permitted (battery saver, insecure context): the wall still works.
			}
		};
		void request();
		document.addEventListener('visibilitychange', request);
		return () => {
			disposed = true;
			document.removeEventListener('visibilitychange', request);
			void lock?.release().catch(() => {});
		};
	});

	// A wall-display account has nowhere else to go: Exit becomes Sign out, and
	// Escape (a remote's Back key, a stray keyboard) does nothing.
	const wallAccount = $derived(auth.isWall);
	function exit() {
		void goto('/');
	}
	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape' && !wallAccount) {
			event.preventDefault();
			exit();
			return;
		}
		// F and T: not while typing and not with a shortcut held.
		if (event.ctrlKey || event.metaKey || event.altKey) return;
		const el = event.target as HTMLElement | null;
		if (el && (el.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName))) return;
		const key = event.key.toLowerCase();
		if (key === 'f') {
			event.preventDefault();
			void toggleFullscreen();
		} else if (key === 't') {
			event.preventDefault();
			themeOverride = wallTheme === 'light' ? 'dark' : 'light';
		}
	}

	// --- This display: options, address parameters, theme, fullscreen, cursor ---
	let prefs = $state<WallPrefs>(readWallPrefs());
	function setPrefs(patch: Partial<WallPrefs>) {
		prefs = { ...prefs, ...patch };
		writeWallPrefs(prefs);
	}
	// `?theme=` and `?scene=` configure one display without touching what is remembered.
	const params = $derived(parseWallParams(page.url.searchParams));
	let themeOverride = $state<'dark' | 'light' | null>(null);
	const wallTheme = $derived(themeOverride ?? params.theme ?? 'dark');

	let fullscreen = $state(false);
	$effect(() => {
		const sync = () => (fullscreen = document.fullscreenElement !== null);
		sync();
		document.addEventListener('fullscreenchange', sync);
		return () => document.removeEventListener('fullscreenchange', sync);
	});
	async function toggleFullscreen() {
		try {
			if (document.fullscreenElement) await document.exitFullscreen();
			else await document.documentElement.requestFullscreen();
		} catch {
			// Refused (no gesture, or not supported): the wall stays as it is.
		}
	}

	// The cursor goes away after 3 s without a move, and comes back with the next one.
	const CURSOR_HIDE_MS = 3000;
	let cursorHidden = $state(false);
	$effect(() => {
		let timer: ReturnType<typeof setTimeout> | undefined;
		const arm = () => {
			cursorHidden = false;
			clearTimeout(timer);
			timer = setTimeout(() => (cursorHidden = true), CURSOR_HIDE_MS);
		};
		arm();
		document.addEventListener('pointermove', arm);
		document.addEventListener('pointerdown', arm);
		return () => {
			clearTimeout(timer);
			document.removeEventListener('pointermove', arm);
			document.removeEventListener('pointerdown', arm);
		};
	});

	// The sound: browsers keep audio locked until a gesture, so the first click, tap or
	// key press unlocks it (the sound button on the wall is such a gesture too).
	$effect(() => {
		if (!prefs.criticalSound) return;
		const unlock = () => unlockWallSound();
		document.addEventListener('pointerdown', unlock, { capture: true });
		document.addEventListener('keydown', unlock, { capture: true });
		return () => {
			document.removeEventListener('pointerdown', unlock, { capture: true });
			document.removeEventListener('keydown', unlock, { capture: true });
		};
	});

	// --- The verdict -----------------------------------------------------------
	const targets = $derived(alertsStore.targets);
	const probes = $derived(alertsStore.probes);
	const firstLoad = $derived(alertsStore.loading && targets.length === 0);
	// Building-up, acknowledged and info alerts stay off the wall: not listed, not counted, no mood.
	const wallAlerts = $derived(visibleOnWall(alertsStore.alerts));
	// A monitor whose only alerts are building up or acknowledged is no problem either.
	const quieted = $derived(quietedOnWall(alertsStore.alerts));
	const sky = $derived(readSky({ targets: targets.filter((t) => !quieted.has(t.id)), probes, alerts: wallAlerts, rules }));
	// The number at the top counts the red problems only; advisories are listed, not counted.
	const critical = $derived(criticalCount(sky.needsYou));
	const verdict = $derived.by(() => {
		if (targets.length === 0) return { tone: 'ghost' as const, text: m.wall_status_empty() };
		if (critical > 0) {
			return {
				tone: 'warning' as const,
				text: critical === 1 ? m.wall_status_problem_one() : m.wall_status_problems({ count: critical })
			};
		}
		if (sky.attention > 0) return { tone: 'advisory' as const, text: m.wall_status_no_critical() };
		if (sky.counts.reporting === 0) return { tone: 'ghost' as const, text: m.wall_status_waiting() };
		return { tone: 'signal' as const, text: m.wall_status_ok() };
	});
	const VERDICT_ICON = { signal: CircleCheck, warning: AlertTriangle, advisory: CircleAlert, ghost: CircleDashed };
	const Icon = $derived(VERDICT_ICON[verdict.tone]);

	const clock = $derived(clockText(now, getLocale(), prefs));
	const dateLabel = $derived(now.toLocaleDateString(getLocale(), { weekday: 'long', day: 'numeric', month: 'long' }));
	// The shared store does not stamp its reads: each refresh replaces its lists, so a change of any is "now".
	let checked = $state<Date | null>(null);
	$effect(() => {
		if (alertsStore.loading) return;
		void alertsStore.alerts;
		void alertsStore.targets;
		void alertsStore.probes;
		checked = new Date();
	});
	const updated = $derived.by(() => {
		if (!checked) return m.wall_updated_waiting();
		const seconds = Math.max(0, Math.round((now.getTime() - checked.getTime()) / 1000));
		return seconds < 90 ? m.wall_updated_seconds({ seconds }) : m.wall_updated_relative({ when: formatRelative(checked) });
	});

	// --- The figures ------------------------------------------------------
	const alertCounts = $derived(countAlertsByTarget(wallAlerts));
	const statuses = $derived(
		targets.map((target) => ({
			target,
			status: quieted.has(target.id) ? ('waiting' as const) : consoleStatus(target, probes.get(target.id), alertCounts.get(target.id))
		}))
	);
	const total = $derived(statuses.filter((r) => r.status !== 'paused').length);
	const up = $derived(statuses.filter((r) => r.status === 'up' || r.status === 'degraded').length);
	const down = $derived(statuses.filter((r) => r.status === 'down').length);
	const paused = $derived(statuses.filter((r) => r.status === 'paused').length);
	const availability = $derived(mean(fleetStore.figures.uptime30.values()));

	const figures = $derived<BandFigure[]>([
		{
			id: 'up',
			label: m.console_kpi_up(),
			value: String(up),
			unit: m.console_kpi_up_unit({ total }),
			note: m.console_kpi_up_note({ down, paused }),
			tone: down > 0 ? 'text-warning-ink' : 'text-ink-2'
		},
		{
			id: 'availability',
			label: m.console_kpi_availability(),
			value: availability === null ? '—' : availability >= 99.9995 ? '100' : availability.toFixed(3),
			unit: availability === null ? '' : '%',
			note: availability === null ? m.console_kpi_none() : m.console_kpi_availability_note({ count: fleetStore.figures.uptime30.size }),
			tone: 'text-ink-2'
		},
		{
			// The headline's own number (critical alerts and monitors down): a count of
			// open alerts alone would leave out a monitor down with no alert, and the
			// foot of the screen would contradict its head.
			id: 'problems',
			label: m.wall_band_problems(),
			value: String(critical),
			unit: '',
			note: m.wall_band_problems_note(),
			tone: critical > 0 ? 'text-warning-ink' : 'text-ink-2'
		}
	]);

	const animate = !calmMotion();

	// --- The world -------------------------------------------------------------
	// Settings sets the default, the Backdrop button changes it for this
	// browser, and a link (`?city=`) decides for one display only.
	let sceneChoice = $state<WallSceneChoice>(readWallScene(DEFAULT_WALL_SCENE));
	const forcedScene = $derived(parseForcedWallScene(page.url.searchParams.get('city')) ?? params.scene);
	// The light theme is a plain, light board: the world is a night picture.
	const sceneCity = $derived(wallTheme === 'light' ? null : resolveWallScene(forcedScene ?? sceneChoice));
	function setScene(choice: WallSceneChoice) {
		sceneChoice = choice;
		writeWallScene(choice);
	}
	// The scene's code comes only when a city is drawn.
	let World = $state.raw<Component<any> | null>(null);
	$effect(() => {
		if (!sceneCity || World) return;
		let alive = true;
		void import('#lib/components/wall/WallWorld.svelte')
			.then((mod) => {
				if (alive) World = mod.default;
			})
			.catch(() => {
				// The chunk could not be fetched: the board stands on its own.
			});
		return () => {
			alive = false;
		};
	});

	// The weather follows the network (`?weather=` or `?mood=` show a state without one).
	const live = useSceneLive();
	const forcedWeather = $derived(parseForcedWeather(page.url.searchParams.get('weather')));
	const weather = $derived<WallWeather>(
		forcedWeather ??
			(live.tone
				? weatherForTone(live.tone)
				: firstLoad
					? 'clear'
					: weatherFor({ attention: sky.attention, warnings: sky.counts.warnings, unreachable: sky.counts.unreachable }))
	);

	// The landmark's gestures (the tower goes dark, comes back, sparkles) follow changes.
	$effect(() => {
		if (alertsStore.loading) {
			live.reset();
			return;
		}
		const downKeys = new Set<string>();
		const issues = new Set<string>();
		for (const row of sky.needsYou) {
			if (row.kind === 'device' && row.tone === 'warning') downKeys.add(row.key);
			else if (row.kind === 'alert' && row.alert.effective_phase === 'firing' && row.alert.rule_uid.startsWith('host_down')) downKeys.add(row.key);
			if (row.tone === 'warning' || row.tone === 'advisory') issues.add(row.key);
		}
		live.feed({ down: downKeys, issues });
	});

	// A new problem startles the pigeons once; nothing startles on the first load.
	let startle = $state(0);
	let attentionBaseline: number | null = null;
	$effect(() => {
		const attention = sky.attention;
		if (alertsStore.loading) return;
		if (attentionBaseline !== null && attention > attentionBaseline) untrack(() => (startle += 1));
		attentionBaseline = attention;
	});

	// A new critical problem beeps once (when the sound is on); the first load is silent.
	let knownCritical: Set<string> | null = null;
	$effect(() => {
		const keys = new Set(sky.needsYou.filter((row) => row.rank === 0).map((row) => row.key));
		if (alertsStore.loading) return;
		const before = knownCritical;
		knownCritical = keys;
		if (before && untrack(() => prefs.criticalSound) && [...keys].some((key) => !before.has(key))) beep();
	});

	let tabHidden = $state(false);
	$effect(() => {
		const sync = () => (tabHidden = document.visibilityState !== 'visible');
		sync();
		document.addEventListener('visibilitychange', sync);
		return () => document.removeEventListener('visibilitychange', sync);
	});
	const sceneStill = $derived(tabHidden || viewport.phone);

	// --- Fitting the problems --------------------------------------------------
	// From 1024 px the board is a fixed-height column: the problems get what the
	// tiles of the monitors below do not need. Below, the page scrolls and
	// nothing is capped.
	const DEVICES_MIN_PX = 56;
	let leftRoom = $state(0);
	let problemListed = $state(new Set<number>());
	let wide = $state(false);
	$effect(() => {
		const query = window.matchMedia('(min-width: 1024px)');
		const sync = () => (wide = query.matches);
		sync();
		query.addEventListener('change', sync);
		return () => query.removeEventListener('change', sync);
	});
	const problemsRoom = $derived(wide && leftRoom > 0 ? Math.max(120, leftRoom - DEVICES_MIN_PX - 16) : Infinity);

	// The tools show for a few seconds when the wall opens, then on hover, focus or touch.
	let toolsHint = $state(true);
	$effect(() => {
		const timer = setTimeout(() => (toolsHint = false), 7000);
		return () => clearTimeout(timer);
	});
</script>

<svelte:head><title>{m.wall_page_title()}</title></svelte:head>
<svelte:window onkeydown={onKeydown} />

<div
	class="wall fixed inset-0 z-[60] box-border bg-canvas pt-[var(--safe-top)] pr-[var(--safe-right)] pb-[var(--safe-bottom)] pl-[var(--safe-left)] text-ink"
	class:has-world={sceneCity !== null}
	class:tokens-night={sceneCity !== null}
	class:sp-preview-light={wallTheme === 'light'}
	class:cursor-hidden={cursorHidden}
>
	<div class="wall-stage relative h-full">
		{#if sceneCity}
			<div class="scene-box" aria-hidden="true">
				{#if World}
					<World
						city={sceneCity}
						{now}
						{weather}
						problems={firstLoad ? 0 : critical}
						{startle}
						paused={sceneStill}
						pulse={live.pulse}
						focus={1360}
					/>
				{/if}
				<div class="scene-fade"></div>
			</div>
			<div class="veil veil-left" aria-hidden="true"></div>
			<div class="veil veil-right" aria-hidden="true"></div>
		{/if}

		<div class="board">
			<div class="board-left" class:rise-in={animate} style="--rise-delay: 60ms">
				{#if firstLoad}
					<div class="grid gap-6">
						<Skeleton class="h-20 w-3/4 lg:h-28" />
						<Skeleton class="h-8 w-1/2" />
						<Skeleton class="h-40 w-full" />
					</div>
				{:else}
					<p class="instance" data-wall-info>{instanceStore.displayName}</p>
					{#key verdict.text}
						<header class="verdict flex items-center gap-4 lg:gap-6" class:verdict-in={animate} data-wall-info>
							<span class="status-mark status-mark--{verdict.tone}" aria-hidden="true">
								<Icon class="size-1/2" strokeWidth={2.4} />
							</span>
							<h1 class="headline min-w-0" aria-live="polite">{verdict.text}</h1>
						</header>
					{/key}
					<WallNotice {now} />
					<div class="board-list mt-7 flex min-h-0 flex-1 flex-col gap-4 lg:mt-9" bind:clientHeight={leftRoom}>
						<WallProblems {sky} {now} room={problemsRoom} bind:listed={problemListed} />
						<WallDevices {targets} {probes} {sky} {quieted} skip={problemListed} rotateSeconds={prefs.rotateSeconds} />
					</div>
				{/if}
			</div>

			<div class="board-right" class:rise-in={animate} style="--rise-delay: 140ms">
				<time class="clock tnum" datetime={now.toISOString()} data-wall-info>{clock}</time>
				<div class="dateline" data-wall-info>
					<p class="date">{dateLabel}</p>
					{#if outdoor?.current}
						<WallOutdoor current={outdoor.current} unit={outdoor.unit} />
					{/if}
				</div>
				<p class="updated tnum" aria-live="off" data-wall-info>
					<span aria-hidden="true" class="size-1.5 rounded-full bg-signal {animate ? 'ob-live' : ''}"></span>
					{updated}
				</p>
			</div>

			<div class="board-band" class:rise-in={animate} style="--rise-delay: 220ms">
				<WallBand {figures} />
			</div>
		</div>

		<!-- Backdrop and Exit: out of sight on a display nobody touches, there on hover, focus or touch. -->
		<div
			class="wall-tools fixed top-3 right-3 z-10 flex items-start gap-1 rounded-xl bg-surface/85 p-1 shadow-lift lg:top-4 lg:right-4"
			class:tools-hint={toolsHint}
			data-wall-info
		>
			<WallSceneControl
				scene={forcedScene ?? sceneChoice}
				sceneForced={forcedScene !== null}
				onscene={setScene}
			/>
			<Button
				variant="ghost"
				size="sm"
				onclick={() => {
					const next = !prefs.criticalSound;
					if (next) unlockWallSound();
					setPrefs({ criticalSound: next });
				}}
				aria-pressed={prefs.criticalSound}
				aria-label={prefs.criticalSound ? m.w1wall_sound_disable() : m.w1wall_sound_enable()}
				title={prefs.criticalSound ? m.w1wall_sound_disable() : m.w1wall_sound_enable()}
				class="min-h-10"
			>
				{#if prefs.criticalSound}
					<Volume2 class="size-4" aria-hidden="true" />
				{:else}
					<VolumeX class="size-4" aria-hidden="true" />
				{/if}
			</Button>
			<Button
				variant="ghost"
				size="sm"
				onclick={() => void toggleFullscreen()}
				aria-label={fullscreen ? m.w1wall_fullscreen_leave() : m.w1wall_fullscreen_enter()}
				title={fullscreen ? m.w1wall_fullscreen_leave() : m.w1wall_fullscreen_enter()}
				class="min-h-10"
			>
				{#if fullscreen}
					<Minimize class="size-4" aria-hidden="true" />
				{:else}
					<Maximize class="size-4" aria-hidden="true" />
				{/if}
			</Button>
			{#if wallAccount}
				<!-- A wall-display account has nowhere else to go: signing out replaces Exit. -->
				<WallSignOut />
			{:else}
				<Button variant="ghost" size="sm" onclick={exit} aria-label={m.wall_exit_aria()} class="min-h-10">
					<X class="size-4" aria-hidden="true" />
					{m.wall_exit()}
					<kbd class="ml-1 rounded-md border border-line-strong bg-surface-2 px-1.5 text-[0.6875rem] text-ink-3 max-sm:hidden">esc</kbd>
				</Button>
			{/if}
		</div>
	</div>
</div>

<style>
	/*
	 * Layout. Phone and tablet: the world is a band at the top and the board
	 * scrolls under it. From 1024 px the world fills the screen and the board
	 * sits in it: verdict and tiles on the left, clock and on-call on the right,
	 * the middle left to the monument, and the foot of the screen to the
	 * parapet, its pigeons and the quiet strip of figures.
	 */
	.wall-stage {
		overflow-y: auto;
	}
	/* No move for 3 s: the pointer goes, over every child too. */
	.cursor-hidden,
	.cursor-hidden :global(*) {
		cursor: none !important;
	}
	.scene-box {
		position: relative;
		height: max(240px, 40vh);
		overflow: hidden;
		isolation: isolate;
	}
	/* On a small screen the band sinks into the page under the headline. */
	.scene-fade {
		position: absolute;
		inset-inline: 0;
		bottom: 0;
		height: 45%;
		background: linear-gradient(180deg, transparent, var(--c-canvas));
	}
	.veil {
		display: none;
	}
	.board {
		display: flex;
		flex-direction: column;
		gap: 2rem;
		padding: 1.5rem 1rem 0;
	}
	/* A world behind: the board lifts over the band's lower edge on a small screen. */
	@media (max-width: 1023px) {
		.has-world .board {
			margin-top: -4rem;
			position: relative;
		}
	}
	.board-left,
	.board-right {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.board-right {
		order: -1;
	}
	.board-band {
		margin-inline: -1rem;
	}
	.instance {
		margin: 0 0 0.5rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 0.9375rem;
		color: var(--c-ink-2);
	}
	.headline {
		margin: 0;
		white-space: nowrap;
		font-size: clamp(2.5rem, 11vw, 3.5rem);
		font-weight: 600;
		line-height: 1;
		letter-spacing: -0.04em;
		color: var(--c-ink);
	}
	.status-mark {
		display: grid;
		place-items: center;
		flex-shrink: 0;
		width: clamp(2.75rem, 12vw, 3.5rem);
		aspect-ratio: 1;
		border-radius: 50%;
		border: 2px solid currentColor;
		background: var(--c-surface);
		box-shadow: var(--shadow-lift);
	}
	.status-mark--signal {
		color: var(--c-signal-ink);
	}
	.status-mark--warning {
		color: var(--c-warning-ink);
	}
	.status-mark--advisory {
		color: var(--c-advisory-ink);
	}
	.status-mark--ghost {
		color: var(--c-ink-2);
	}
	.clock {
		font-size: 3.25rem;
		font-weight: 600;
		letter-spacing: -0.04em;
		line-height: 1;
		color: var(--c-ink);
	}
	.dateline {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.25rem 1rem;
		margin-top: 0.375rem;
		font-size: 1.25rem;
	}
	.date {
		margin: 0;
		font-size: inherit;
		font-weight: 600;
		color: var(--c-ink-2);
		text-transform: capitalize;
	}
	.updated {
		margin: 0.5rem 0 0;
		display: flex;
		align-items: center;
		gap: 0.625rem;
		font-family: var(--font-code, ui-monospace, monospace);
		font-size: 0.8125rem;
		color: var(--c-ink-2);
	}
	/* The verdict changes in a short, soft arrival, never a flash. */
	.verdict-in {
		animation: verdict-in 240ms cubic-bezier(0.2, 0, 0, 1) both;
	}
	@keyframes verdict-in {
		from {
			opacity: 0;
			transform: translateY(6px);
		}
	}

	@media (min-width: 1024px) {
		.wall-stage {
			overflow: hidden;
		}
		.scene-box {
			position: absolute;
			inset: 0;
			height: auto;
		}
		.scene-fade {
			display: none;
		}
		/* A veil behind each column's words: legible over any hour, any cloud. */
		.veil {
			display: block;
			position: absolute;
			inset-block: 0;
			pointer-events: none;
		}
		.veil-left {
			left: 0;
			width: min(54rem, 52vw);
			background: linear-gradient(
				90deg,
				color-mix(in srgb, var(--c-canvas) 64%, transparent),
				color-mix(in srgb, var(--c-canvas) 44%, transparent) 62%,
				transparent
			);
		}
		.veil-right {
			right: 0;
			width: min(34rem, 32vw);
			background: linear-gradient(
				270deg,
				color-mix(in srgb, var(--c-canvas) 60%, transparent),
				color-mix(in srgb, var(--c-canvas) 38%, transparent) 62%,
				transparent
			);
		}
		.board {
			position: absolute;
			inset: 0;
			margin-top: 0;
			display: grid;
			grid-template-columns: minmax(0, 38rem) minmax(0, 1fr) minmax(0, 24rem);
			grid-template-rows: minmax(0, 1fr) auto;
			gap: 0 2rem;
			/* The words come first: the world's props give way to them, never the reverse. */
			padding: 3rem 3.5rem 0;
		}
		.board-left {
			grid-column: 1;
			grid-row: 1;
			min-height: 0;
		}
		.board-right {
			order: 0;
			grid-column: 3;
			grid-row: 1;
			align-items: flex-end;
			text-align: right;
			padding-top: 1.75rem;
			min-height: 0;
		}
		.board-band {
			grid-column: 1 / -1;
			grid-row: 2;
			margin-inline: -3.5rem;
		}
		.headline {
			font-size: clamp(4.25rem, 5vw, 6rem);
		}
		.status-mark {
			width: clamp(4rem, 4.8vw, 5.75rem);
			border-width: 3px;
		}
		.clock {
			font-size: 6.5rem;
		}
		.dateline {
			justify-content: flex-end;
			font-size: 1.75rem;
		}
		.updated {
			font-size: 0.9375rem;
		}
	}
	/* The words sit on a scene: a soft halo keeps them readable over its details. */
	.has-world .headline,
	.has-world .clock,
	.has-world .dateline,
	.has-world .updated,
	.has-world .instance {
		text-shadow: 0 0 14px color-mix(in srgb, var(--c-canvas) 85%, transparent);
	}
	/* 720p to 900p: a narrower board leaves the middle of the scene to its monument. */
	@media (min-width: 1024px) and (max-width: 1599px) {
		.board {
			grid-template-columns: minmax(0, 27rem) minmax(0, 1fr) minmax(0, 20rem);
			gap: 0 1.5rem;
			padding-inline: 2.5rem;
		}
		.board-band {
			margin-inline: -2.5rem;
		}
		.headline {
			font-size: clamp(3.5rem, 5.4vw, 4.5rem);
		}
		.clock {
			font-size: 4.75rem;
		}
	}
	/* A short screen: the right column trims its spacing so the tiles keep their room. */
	@media (min-width: 1024px) and (max-height: 940px) {
		.board {
			padding-top: 2.25rem;
		}
		.instance,
		.updated {
			display: none;
		}
		.dateline {
			font-size: 1.25rem;
			margin-top: 0.125rem;
		}
		.board-list {
			margin-top: 1.25rem;
		}
	}
	/* A 4K panel read from the same sofa: the board grows with the screen. */
	@media (min-width: 2400px) {
		.board,
		.wall-tools {
			zoom: 1.33;
		}
	}
	@media (min-width: 3200px) {
		.board,
		.wall-tools {
			zoom: 2;
		}
	}

	/* Backdrop and Exit: out of sight on a display nobody touches. */
	.wall-tools {
		opacity: 0;
		transition: opacity 200ms ease;
	}
	.wall:hover .wall-tools,
	.wall-tools:focus-within,
	.wall-tools:has([aria-expanded='true']),
	.wall-tools.tools-hint {
		opacity: 1;
	}
	@media (hover: none) {
		.wall-tools {
			opacity: 1;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.wall-tools {
			transition: none;
		}
		.verdict-in {
			animation: none;
		}
	}
</style>
