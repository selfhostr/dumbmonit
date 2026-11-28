<script lang="ts">
	/** The status page editor belongs to the optional "status pages" module (Settings → System → Modules). */
	import type { Snippet } from 'svelte';
	import ModuleOff from '#lib/components/shell/ModuleOff.svelte';
	import { featuresStore } from '#lib/stores/features.svelte.js';

	let { children }: { children: Snippet } = $props();
</script>

{#if featuresStore.on('status_pages')}
	{@render children()}
{:else if featuresStore.loaded}
	<ModuleOff />
{/if}
