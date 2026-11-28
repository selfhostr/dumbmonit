<script lang="ts">
	import { docTitle } from '#lib/stores/page-title.svelte.js';
	/**
	 * Setup wizard, after the first administrator signs in (and again from
	 * Settings → General). It opens on "Your first alert": one thing to watch,
	 * where the alert goes and a test that proves it, the whole road from a
	 * fresh instance to a message on the phone. Then name and language, more
	 * monitors, an agent, the optional modules, and three optional steps
	 * (single sign-on and teams, on-call, retention).
	 *
	 * One step per page, full screen, no app shell: the step and its counter on
	 * top, Back / Continue in a footer that stays in view. Enter outside a form
	 * means Continue. "Skip setup" ends it at once. The step lives in the URL
	 * (`?step=monitors`), so the browser's Back works and a language change,
	 * which reloads the page, lands on the next step. Finishing marks the
	 * wizard done on the server, for every browser.
	 */
	import { tick } from 'svelte';
	import { goto } from '#lib/navigation.js';
	import { page } from '$app/state';
	import { getInstanceSettings, updateInstanceSettings } from '#lib/api/instance.js';
	import { getFeatures, updateFeatures, updateOnboarding, type Features } from '#lib/api/index.js';
	import { m } from '#lib/paraglide/messages.js';
	import { getLocale, setLocale, type Locale } from '#lib/paraglide/runtime.js';
	import { auth } from '#lib/stores/auth.svelte.js';
	import { instanceStore } from '#lib/stores/instance.svelte.js';
	import { Button, ErrorNotice } from '#lib/ui/index.js';
	import Logo from '#lib/components/Logo.svelte';
	import InstanceFields from '#lib/components/modes/InstanceFields.svelte';
	import { browserTimeZone } from '#lib/components/modes/timezones.js';
	import WizardQuickStart from '#lib/components/wizard/WizardQuickStart.svelte';
	import WizardMonitors from '#lib/components/wizard/WizardMonitors.svelte';
	import WizardOnCall from '#lib/components/wizard/WizardOnCall.svelte';
	import WizardPigeon from '#lib/components/wizard/WizardPigeon.svelte';
	import WizardModules from '#lib/components/wizard/WizardModules.svelte';
	import AgentEnroll from '#lib/components/device-form/AgentEnroll.svelte';
	import SsoSection from '#lib/components/settings/SsoSection.svelte';
	import UsersSection from '#lib/components/settings/UsersSection.svelte';
	import StorageSection from '#lib/components/settings/StorageSection.svelte';

	type StepId = 'start' | 'instance' | 'monitors' | 'agent' | 'modules' | 'sso' | 'oncall' | 'retention';
	interface Step {
		id: StepId;
		title: () => string;
		text: () => string;
		/** What the pigeon says on this step. */
		say: () => string;
		optional?: boolean;
		/** Choice grids and tables take the wide column; forms keep a reading width. */
		wide?: boolean;
	}

	const STEPS: Step[] = [
		{ id: 'start', title: () => m.wizard_start_title(), text: () => m.wizard_start_text(), say: () => m.wizard_pigeon_start() },
		{ id: 'instance', title: () => m.wizard_instance_title(), text: () => m.wizard_instance_text(), say: () => m.wizard_pigeon_instance() },
		{ id: 'monitors', title: () => m.wizard_monitors_title(), text: () => m.wizard_monitors_text(), say: () => m.wizard_pigeon_monitors(), wide: true },
		{ id: 'agent', title: () => m.wizard_agent_title(), text: () => m.wizard_agent_text(), say: () => m.wizard_pigeon_agent(), wide: true },
		{ id: 'modules', title: () => m.wizard_modules_title(), text: () => m.wizard_modules_text(), say: () => m.wizard_pigeon_modules() },
		{ id: 'sso', title: () => m.wizard_sso_title(), text: () => m.wizard_sso_text(), say: () => m.wizard_pigeon_sso(), optional: true, wide: true },
		{ id: 'oncall', title: () => m.wizard_oncall_title(), text: () => m.wizard_oncall_text(), say: () => m.wizard_pigeon_oncall(), optional: true },
		{ id: 'retention', title: () => m.wizard_retention_title(), text: () => m.wizard_retention_text(), say: () => m.wizard_pigeon_retention(), optional: true, wide: true }
	];

	// On-call schedules are optional (Settings → System): their step only shows while they are on.
	const steps = $derived(STEPS.filter((s) => s.id !== 'oncall' || instanceStore.oncallEnabled));
	// `channel` was the notification step before it joined "Your first alert".
	const asked = $derived(page.url.searchParams.get('step') === 'channel' ? 'start' : page.url.searchParams.get('step'));
	const index = $derived(Math.max(0, steps.findIndex((s) => s.id === asked)));
	const step = $derived(steps[index]);
	const last = $derived(index === steps.length - 1);
	const progress = $derived(((index + 1) / steps.length) * 100);
	/** Happy on the last step: after it, the rounds start. */
	const pigeonMood = $derived(last ? 'happy' : 'watch');

	let name = $state('');
	let language = $state('');
	let timezone = $state('');
	let busy = $state(false);
	let error = $state<unknown>(null);
	let heading = $state<HTMLHeadingElement | null>(null);
	// Optional modules: all off until the server says otherwise.
	let features = $state<Features>({ wall: false, sla_reports: false, status_pages: false, slo: false, threat_map: false });

	$effect(() => {
		const controller = new AbortController();
		getFeatures(controller.signal)
			.then((saved) => (features = { ...features, ...saved }))
			.catch(() => {});
		return () => controller.abort();
	});

	$effect(() => {
		getInstanceSettings()
			.then((settings) => {
				name = settings.name;
				language = settings.language;
				timezone = settings.timezone || browserTimeZone();
			})
			.catch(() => {
				timezone = browserTimeZone();
			});
	});

	$effect(() => {
		void instanceStore.loadSettings();
	});

	// Only administrators set the instance up.
	$effect(() => {
		if (auth.canUseApi && !auth.isAdmin) void goto('/', { replace: true });
	});

	// Each new step takes the focus to its title, so a screen reader hears where it is.
	$effect(() => {
		step;
		void tick().then(() => heading?.focus());
	});

	function go(target: number) {
		const next = steps[Math.max(0, Math.min(steps.length - 1, target))];
		error = null;
		void goto(`/welcome?step=${next.id}`, { reset: false }).then(() => window.scrollTo({ top: 0 }));
	}

	/** Saves what the step holds, then moves on. */
	async function advance() {
		if (busy) return;
		busy = true;
		error = null;
		try {
			if (step.id === 'modules') {
				features = { ...features, ...(await updateFeatures({ ...features })) };
			}
			if (step.id === 'instance') {
				const settings = await updateInstanceSettings({ name: name.trim(), language, timezone });
				instanceStore.name = settings.name;
				if (settings.language && settings.language !== getLocale()) {
					// A new language reloads the page: land on the next step.
					await goto(`/welcome?step=${steps[index + 1].id}`, { replace: true });
					setLocale(settings.language as Locale);
					return;
				}
			}
			if (last) await finish();
			else go(index + 1);
		} catch (cause) {
			error = cause;
		} finally {
			busy = false;
		}
	}

	/**
	 * Enter means Continue, unless it already does something where it was
	 * pressed: a form (it submits), a text area, a button or link, a menu or a dialog.
	 */
	function onkeydown(event: KeyboardEvent) {
		if (event.key !== 'Enter' || event.defaultPrevented || event.isComposing) return;
		if (event.shiftKey || event.altKey || event.ctrlKey || event.metaKey) return;
		const target = event.target instanceof Element ? event.target : null;
		if (target?.closest('form, textarea, button, a, select, summary, [contenteditable], [role="combobox"], [role="listbox"], [role="menu"], [role="switch"], [role="dialog"], dialog')) return;
		event.preventDefault();
		void advance();
	}

	/** Marks the wizard done (and the overview's three-step guide with it), then opens the app. */
	async function finish() {
		busy = true;
		error = null;
		try {
			await updateInstanceSettings({ setup_completed: true });
			instanceStore.setupCompleted = true;
			await updateOnboarding({ skipped: true }).catch(() => {});
			await goto('/', { replace: true });
		} catch (cause) {
			error = cause;
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>{docTitle(m.wizard_title())}</title></svelte:head>
<svelte:window {onkeydown} />

<!-- The whole screen, no app shell: header, one step centred in the free space, footer always in view. -->
<div class="flex min-h-dvh flex-1 flex-col bg-canvas text-ink">
	<header class="relative border-b border-line bg-canvas">
		<div class="mx-auto flex h-14 w-full max-w-5xl items-center gap-3 px-4 sm:px-6">
			<Logo class="size-7 shrink-0" />
			<span class="min-w-0 truncate font-semibold">{m.wizard_title()}</span>
			<Button variant="ghost" size="sm" class="ml-auto !h-10 shrink-0" disabled={busy} onclick={finish}>{m.wizard_skip_all()}</Button>
		</div>
		<!-- Progress: a hairline along the header's foot; the counter above the title says it in words. -->
		<div class="absolute inset-x-0 -bottom-px h-0.5" aria-hidden="true">
			<div class="progress-ink h-full bg-ink transition-[width] duration-500 ease-out-expo" style={`width: ${progress}%`}></div>
		</div>
	</header>

	<main class="flex-1 px-4 pt-8 pb-10 sm:px-6 sm:pt-14 sm:pb-16">
		{#key step.id}
			<section class={`rise-in mx-auto w-full min-w-0 ${step.wide ? 'max-w-5xl' : 'max-w-3xl'}`} aria-labelledby="wizard-step-title">
				<p class="text-[0.8125rem] text-ink-2">
					{m.wizard_step_of({ current: index + 1, total: steps.length })}
					{#if step.optional}<span class="text-ink-3"> · {m.wizard_optional()}</span>{/if}
				</p>
				<h1 id="wizard-step-title" bind:this={heading} tabindex="-1" class="display mt-2 text-3xl break-words text-ink outline-none max-sm:text-2xl">
					{step.title()}
				</h1>
				<p class="mt-3 max-w-[65ch] break-words text-ink-2">{step.text()}</p>
				{#if index === 0}<p class="mt-1 max-w-[65ch] text-[0.8125rem] break-words text-ink-3">{m.wizard_intro()}</p>{/if}
				<WizardPigeon class="mt-6 max-w-lg max-sm:hidden" message={step.say()} mood={pigeonMood} />

				<div class="mt-8 min-w-0">
					{#if step.id === 'start'}
						<WizardQuickStart />
					{:else if step.id === 'instance'}
						<InstanceFields bind:name bind:language bind:timezone idPrefix="wizard-instance" />
					{:else if step.id === 'monitors'}
						<WizardMonitors />
					{:else if step.id === 'agent'}
						<AgentEnroll cancelHref={`/welcome?step=${steps[index + 1]?.id ?? 'agent'}`} />
					{:else if step.id === 'modules'}
						<WizardModules bind:value={features} disabled={busy} />
					{:else if step.id === 'sso'}
						<div class="grid min-w-0 gap-6">
							<SsoSection />
							<UsersSection />
						</div>
					{:else if step.id === 'oncall'}
						<WizardOnCall {timezone} />
					{:else if step.id === 'retention'}
						<StorageSection />
					{/if}
				</div>

				{#if error}<ErrorNotice {error} class="mt-6" />{/if}
			</section>
		{/key}
	</main>

	<footer class="sticky bottom-0 z-20 border-t border-line bg-canvas pb-[var(--safe-bottom)]">
		<div class="mx-auto flex w-full max-w-5xl flex-wrap items-center gap-2 px-4 py-3 sm:px-6">
			{#if index > 0}
				<Button variant="ghost" class="!h-10" disabled={busy} onclick={() => go(index - 1)}>{m.wizard_back()}</Button>
			{/if}
			<div class="ml-auto flex min-w-0 flex-wrap items-center justify-end gap-2">
				{#if step.optional}
					<Button variant="ghost" class="!h-10" disabled={busy} onclick={() => (last ? finish() : go(index + 1))}>{m.wizard_skip_step()}</Button>
				{/if}
				<Button variant="primary" class="!h-10" loading={busy} onclick={advance}>{last ? m.wizard_finish() : m.wizard_next()}</Button>
			</div>
		</div>
	</footer>
</div>

<style>
	/* The title takes the focus on every step, for screen readers; it is not a control, so no ring. */
	#wizard-step-title:focus-visible {
		outline: none;
	}
</style>
