<script lang="ts">
	/**
	 * Toggle chip for filters: a pressed state shown by fill, a check and
	 * `aria-pressed`, never by colour alone. 40px tall for a thumb.
	 */
	import type { Snippet } from 'svelte';
	import { Check } from 'lucide-svelte';

	interface Props {
		pressed?: boolean;
		disabled?: boolean;
		onclick: () => void;
		children: Snippet;
		/** Figure shown after the label, such as a number of results. */
		count?: number;
		class?: string;
	}

	let { pressed = false, disabled = false, onclick, children, count, class: className = '' }: Props = $props();
</script>

<button
	type="button"
	aria-pressed={pressed}
	{disabled}
	{onclick}
	class={`inline-flex min-h-10 shrink-0 items-center gap-1.5 rounded-full border px-3 text-[0.8125rem] font-medium whitespace-nowrap transition-colors duration-150 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal disabled:cursor-not-allowed disabled:opacity-45 ${
		pressed
			? 'border-signal bg-signal-soft text-signal-ink'
			: 'border-line bg-surface text-ink-2 enabled:hover:border-line-strong enabled:hover:text-ink'
	} ${className}`}
>
	{#if pressed}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
	{@render children()}
	{#if count !== undefined}<span class="text-[0.75rem] font-normal tabular-nums text-ink-3">{count}</span>{/if}
</button>
