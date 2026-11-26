<script lang="ts" module>
	export interface SegmentOption {
		value: string;
		label: string;
	}
</script>

<script lang="ts">
	/**
	 * One choice among a few, side by side: native radio buttons styled as a
	 * segmented control, so the browser gives the radio-group keyboard (one
	 * tab stop, arrows move and choose). A `fieldset` whose legend reads like a
	 * `Field` label, with the help line under it. The control is as tall as an
	 * `.input` (40px), and each segment's hit area covers that whole height.
	 * The chosen segment is shown by a raised surface and bold text, not by
	 * colour alone.
	 */
	interface Props {
		legend: string;
		options: readonly SegmentOption[];
		/** Bindable. */
		value: string;
		/** Name of the radio group, unique on the page. */
		name: string;
		help?: string;
		disabled?: boolean;
		/** The legend is read by screen readers only. */
		hideLegend?: boolean;
		class?: string;
		onchange?: (value: string) => void;
	}

	let { legend, options, value = $bindable(), name, help, disabled = false, hideLegend = false, class: className = '', onchange }: Props = $props();
</script>

<fieldset class={`grid min-w-0 content-start gap-1.5 ${className}`} {disabled}>
	<legend class={hideLegend ? 'sr-only' : 'mb-1.5 block text-sm font-semibold text-ink'}>{legend}</legend>
	<div class="flex min-h-10 w-fit max-w-full flex-wrap items-center gap-1 rounded-lg border border-line-strong bg-canvas-deep p-1">
		{#each options as option (option.value)}
			<label
				class={`relative inline-flex h-8 shrink-0 cursor-pointer items-center rounded-md px-3 text-sm whitespace-nowrap transition-colors before:absolute before:inset-x-0 before:-inset-y-1 before:content-[''] has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-signal has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-50 ${value === option.value ? 'bg-surface font-semibold text-ink shadow-lift' : 'text-ink-2 hover:text-ink'}`}
			>
				<input
					type="radio"
					class="sr-only"
					{name}
					value={option.value}
					checked={value === option.value}
					onchange={() => {
						value = option.value;
						onchange?.(option.value);
					}}
				/>
				{option.label}
			</label>
		{/each}
	</div>
	{#if help}<p class="text-[0.8125rem] text-ink-2">{help}</p>{/if}
</fieldset>
