<script lang="ts">
	/**
	 * Settings → Wall display: what the room monitor is, the way in, and what it
	 * shows by default in this browser: the backdrop behind the board (Paris unless
	 * chosen otherwise). The wall is always Night. The wall's own Backdrop button
	 * changes the same choice, and a link (`?city=`) overrides it for one display.
	 */
	import { Tv } from 'lucide-svelte';
	import { m } from '#lib/paraglide/messages.js';
	import { Button, DocLink, Panel } from '#lib/ui/index.js';
	import { wallSceneOptions } from '#lib/components/wall/wallOptions.js';
	import { DEFAULT_WALL_SCENE, readWallScene, writeWallScene, type WallSceneChoice } from '#lib/components/wall/wallScene.js';

	let scene = $state<WallSceneChoice>(readWallScene(DEFAULT_WALL_SCENE));

	const SCENE_OPTIONS = $derived(wallSceneOptions());

	function pickScene(value: WallSceneChoice) {
		scene = value;
		writeWallScene(value);
	}

	/** Arrows move the choice, as in any radio group. */
	function arrow(event: KeyboardEvent, index: number, count: number, pick: (index: number) => void) {
		const delta = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 0;
		if (!delta) return;
		event.preventDefault();
		const next = (index + delta + count) % count;
		pick(next);
		(event.currentTarget as HTMLElement).parentElement?.querySelectorAll<HTMLElement>('[role="radio"]')[next]?.focus();
	}

	const CHIP = 'min-h-10 rounded-[var(--radius-plate)] border px-3 py-1 text-[0.8125rem] transition-colors';
	const CHIP_ON = 'border-signal bg-signal-soft font-semibold text-ink';
	const CHIP_OFF = 'border-line text-ink-2 hover:bg-surface-2';
</script>

<Panel id="wall" title={m.settings_appearance_wall_title()} description={m.settings_appearance_wall_description()}>
	<div class="grid gap-5">
		<div class="grid gap-2" role="group" aria-labelledby="wall-default-scene">
			<p id="wall-default-scene" class="m-0 text-sm font-semibold text-ink">{m.settings_wall_scene_label()}</p>
			<div class="flex flex-wrap gap-1.5" role="radiogroup" aria-labelledby="wall-default-scene">
				{#each SCENE_OPTIONS as option, i (option.value)}
					<button
						type="button"
						role="radio"
						aria-checked={scene === option.value}
						tabindex={scene === option.value ? 0 : -1}
						class={`${CHIP} ${scene === option.value ? CHIP_ON : CHIP_OFF}`}
						onclick={() => pickScene(option.value)}
						onkeydown={(event) => arrow(event, i, SCENE_OPTIONS.length, (next) => pickScene(SCENE_OPTIONS[next].value))}>{option.label}</button
					>
				{/each}
			</div>
		</div>

		<p class="m-0 text-[0.8125rem] text-ink-3">{m.settings_wall_remembered()}</p>

		<div>
			<Button href="/wall" variant="secondary">
				<Tv class="size-4" aria-hidden="true" />
				{m.settings_appearance_wall_open()}
			</Button>
		</div>
	</div>
	<DocLink page="using/wall/" class="mt-3" />
</Panel>
