<script lang="ts">
	/**
	 * Wall mode: the room monitor, a picture (Paris and its pigeons by default,
	 * under the real sky and the network's weather) with the verdict, the clock,
	 * the problems and the monitors on top, and the fleet figures along the foot.
	 * An optional module: off, a notice stands in its place.
	 */
	import EnterpriseWall from '#lib/components/wall/EnterpriseWall.svelte';
	import ModuleOff from '#lib/components/shell/ModuleOff.svelte';
	import { featuresStore } from '#lib/stores/features.svelte.js';
</script>

{#if featuresStore.on('wall')}
	<EnterpriseWall />
{:else if featuresStore.loaded}
	<ModuleOff />
{/if}
