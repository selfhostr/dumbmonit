<script lang="ts">
	/**
	 * What a page of an optional module shows while the module is off
	 * (Settings → System → Modules): administrators get a short notice with the
	 * way to turn it on, everyone else the usual "not found".
	 */
	import { Compass, Puzzle } from 'lucide-svelte';
	import { m } from '#lib/paraglide/messages.js';
	import { auth } from '#lib/stores/auth.svelte.js';
	import { Button, EmptyState } from '#lib/ui/index.js';
</script>

<div class="mx-auto max-w-lg py-16">
	{#if auth.isAdmin}
		<EmptyState icon={Puzzle} title={m.modules_off_title()} description={m.modules_off_text()}>
			{#snippet action()}
				<Button href="/settings/system#modules" variant="primary">{m.modules_off_action()}</Button>
			{/snippet}
		</EmptyState>
	{:else}
		<EmptyState icon={Compass} mascot="dizzy" title={m.error_page_heading_not_found()} description={m.error_page_description_not_found()}>
			{#snippet action()}
				<Button href="/" variant="primary">{m.error_page_back()}</Button>
			{/snippet}
		</EmptyState>
	{/if}
</div>
