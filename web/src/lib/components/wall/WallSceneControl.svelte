<script lang="ts">
	/**
	 * "Backdrop" on the wall: a quiet button beside Exit, opening a small panel
	 * that chooses the world behind the board (Paris or a plain board). Remembered
	 * per browser, and the same choice Settings sets as the default; a `?city=` on
	 * a shared link overrides it for that display without touching what is saved.
	 * Escape or a click elsewhere closes the panel (not the wall).
	 */
	import { m } from '#lib/paraglide/messages.js';
	import { Building2 } from 'lucide-svelte';
	import { Button } from '#lib/ui/index.js';
	import { wallSceneOptions } from '#lib/components/wall/wallOptions.js';
	import type { WallSceneChoice } from '#lib/components/wall/wallScene.js';

	interface Props {
		scene: WallSceneChoice;
		/** True when a `?city=` link is choosing right now. */
		sceneForced: boolean;
		onscene: (value: WallSceneChoice) => void;
	}

	let { scene, sceneForced, onscene }: Props = $props();

	let open = $state(false);
	let root = $state<HTMLDivElement | null>(null);

	const SCENE_OPTIONS = $derived(wallSceneOptions());
	$effect(() => {
		if (!open) return;
		const onPointer = (event: PointerEvent) => {
			if (root && !root.contains(event.target as Node)) open = false;
		};
		const onKey = (event: KeyboardEvent) => {
			if (event.key === 'Escape') {
				event.stopPropagation();
				open = false;
			}
		};
		document.addEventListener('pointerdown', onPointer);
		window.addEventListener('keydown', onKey, true);
		return () => {
			document.removeEventListener('pointerdown', onPointer);
			window.removeEventListener('keydown', onKey, true);
		};
	});

	function toggle() {
		open = !open;
	}

	/** Arrows move the choice, as in any radio group. */
	function arrow(event: KeyboardEvent, index: number, count: number, pick: (index: number) => void) {
		const delta = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 0;
		if (!delta) return;
		event.preventDefault();
		const next = (index + delta + count) % count;
		pick(next);
		const group = (event.currentTarget as HTMLElement).parentElement;
		group?.querySelectorAll<HTMLElement>('[role="radio"]')[next]?.focus();
	}

	const CHIP = 'min-h-10 rounded-[var(--radius-plate)] border px-3 py-1 text-[0.8125rem] transition-colors';
	const CHIP_ON = 'border-signal bg-signal-soft font-semibold text-ink';
	const CHIP_OFF = 'border-line text-ink-2 hover:bg-surface-2';
</script>

<div class="relative flex items-start gap-1" bind:this={root}>
	<Button variant="ghost" size="sm" onclick={toggle} aria-expanded={open} aria-controls="wall-scene-panel" class="min-h-10">
		<Building2 class="size-4" aria-hidden="true" />
		{m.wall_scene_button()}
	</Button>
	{#if open}
		<div
			id="wall-scene-panel"
			class="absolute top-full right-0 z-20 mt-2 w-[min(calc(100vw-2rem),22rem)] rounded-[var(--radius-card)] border border-line bg-surface p-4 text-left shadow-float"
			role="dialog"
			aria-label={m.wall_scene_world_title()}
		>
			<p class="m-0 text-sm font-semibold text-ink">{m.wall_scene_world_title()}</p>
			<p class="m-0 mt-1 text-[0.8125rem] text-ink-2">{sceneForced ? m.wall_scene_forced() : m.wall_scene_world_hint()}</p>
			<div class="mt-3 flex flex-wrap gap-1.5" role="radiogroup" aria-label={m.wall_scene_world_title()}>
				{#each SCENE_OPTIONS as option, i (option.value)}
					<button
						type="button"
						role="radio"
						aria-checked={scene === option.value}
						tabindex={scene === option.value ? 0 : -1}
						class={`${CHIP} ${scene === option.value ? CHIP_ON : CHIP_OFF}`}
						onclick={() => onscene(option.value)}
						onkeydown={(event) => arrow(event, i, SCENE_OPTIONS.length, (next) => onscene(SCENE_OPTIONS[next].value))}>{option.label}</button
					>
				{/each}
			</div>
		</div>
	{/if}
</div>
