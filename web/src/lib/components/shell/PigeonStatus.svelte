<script lang="ts">
	/**
	 * The pigeon in the header, small, reading the network: asleep when all is
	 * well, watching when something needs a look, on alert (pinprick eyes and a
	 * "!" badge) when a monitor is down. Beside it, one line of status in words
	 * ("pve-02 is down.", "All 146 monitors up."), worst news first. The state
	 * is in text and in a shape, never in colour alone. It hops once when its state changes; under reduced motion
	 * it only changes face.
	 */
	import { countAlertsByTarget } from '#lib/alerts-attention.js';
	import { m } from '#lib/paraglide/messages.js';
	import { alertsStore } from '#lib/stores/alerts.svelte.js';
	import { reducedMotion } from '#lib/ui/index.js';
	import Mascot from '#lib/components/Mascot.svelte';
	import { bannerLine, type BannerLine } from '#lib/components/enterprise/banner.js';
	import { consoleStatus } from '#lib/components/enterprise/status.js';

	type Mood = 'sleep' | 'watch' | 'alert';

	/** What the status line says, in the reader's language (worst news first, see `banner.ts`). */
	function words(line: BannerLine): { headline: string; sub: string } {
		switch (line.kind) {
			case 'empty':
				return { headline: m.console_banner_empty(), sub: '' };
			case 'down':
				return {
					headline: line.name !== null ? m.console_banner_down_one({ name: line.name }) : m.console_banner_down_many({ count: line.count }),
					sub:
						line.held > 0
							? line.sent
								? m.console_banner_held_sent({ count: line.held })
								: m.console_banner_held({ count: line.held })
							: line.sent
								? m.console_banner_sent()
								: ''
				};
			case 'attention':
				if (line.count === 0) return { headline: m.console_banner_alerts({ count: line.alerts }), sub: '' };
				return {
					headline: line.name !== null ? m.console_banner_attention_one({ name: line.name }) : m.console_banner_attention_many({ count: line.count }),
					sub: line.alerts > 0 ? m.console_banner_alerts({ count: line.alerts }) : ''
				};
			case 'waiting':
				return { headline: m.console_banner_up_of({ up: line.up, total: line.total }), sub: m.console_banner_waiting({ count: line.waiting }) };
			case 'up':
				if (line.count === 0) return { headline: m.console_banner_all_paused(), sub: '' };
				return { headline: m.console_banner_all_up({ count: line.count }), sub: line.paused > 0 ? m.console_banner_paused({ count: line.paused }) : '' };
		}
	}

	/** Nothing read yet: the line waits rather than say "nothing to watch". */
	const reading = $derived(alertsStore.loading && alertsStore.targets.length === 0);
	const line = $derived.by(() => {
		const counts = countAlertsByTarget(alertsStore.alerts);
		const monitors = alertsStore.targets.map((target) => ({
			id: target.id,
			name: target.name,
			status: consoleStatus(target, alertsStore.probes.get(target.id), counts.get(target.id))
		}));
		return bannerLine(monitors, alertsStore.alerts);
	});
	const text = $derived(reading ? { headline: '', sub: '' } : words(line));
	const mood = $derived<Mood>(
		reading ? 'watch' : line.kind === 'down' ? 'alert' : line.kind === 'attention' || line.kind === 'waiting' ? 'watch' : 'sleep'
	);

	const label = $derived(
		mood === 'alert' ? m.shell_pigeon_alert() : mood === 'watch' ? m.shell_pigeon_watch() : m.shell_pigeon_sleep()
	);

	let host = $state<HTMLSpanElement | null>(null);
	let previous: Mood | null = null;
	$effect(() => {
		const next = mood;
		if (previous !== null && previous !== next && host && !reducedMotion()) {
			host.animate(
				[{ transform: 'translateY(0)' }, { transform: 'translateY(-3px)', offset: 0.4 }, { transform: 'translateY(0)' }],
				{ duration: 280, easing: 'cubic-bezier(0.2, 0, 0, 1)' }
			);
		}
		previous = next;
	});
</script>

<div role="status" data-tour="weather" class="flex min-w-0 shrink items-center gap-2">
	<span bind:this={host} class="relative inline-flex size-8 shrink-0 items-center justify-center" title={label}>
		<Mascot mood={mood} {label} blink={false} class="size-7" />
		{#if mood === 'alert'}
			<span
				aria-hidden="true"
				class="state-in absolute -top-0.5 -right-0.5 grid size-3.5 place-items-center rounded-full bg-warning font-mono text-[0.5625rem] leading-none font-bold text-canvas"
				>!</span
			>
		{/if}
	</span>
	<!-- Phones keep the pigeon alone (its face and badge carry the state); the sentence stays for screen readers. -->
	<span
		class="sr-only md:not-sr-only md:block md:max-w-[12rem] md:truncate md:text-sm md:font-medium md:text-ink lg:max-w-[14rem] xl:max-w-[26rem]"
		title={text.sub ? `${text.headline} ${text.sub}` : text.headline}
		>{text.headline}{#if text.sub}<span class="sr-only"> {text.sub}</span>{/if}</span
	>
</div>
