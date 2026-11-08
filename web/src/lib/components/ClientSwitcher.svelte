<script lang="ts">
	/**
	 * MSP mode: which client the pages show. The MSP administrator picks one
	 * client or all of them; a member of several clients picks among his own.
	 * The choice travels with every request (`x-dumbmonit-client`): the server
	 * narrows the lists, it never widens anyone's access. Hidden when there is
	 * nothing to pick. In the top bar it shrinks to an icon (all clients) or the
	 * picked client's name.
	 */
	import { Building2 } from 'lucide-svelte';
	import { auth } from '#lib/stores/auth.svelte.js';
	import { m } from '#lib/paraglide/messages.js';

	interface Props {
		/** The phone strip: a full-width select. Off, the compact top-bar form. */
		wide?: boolean;
		id?: string;
	}
	let { wide = false, id = 'client-switcher' }: Props = $props();

	const clients = $derived(auth.msp?.clients ?? []);
	const admin = $derived(auth.msp?.admin === true);
	const shown = $derived(auth.mspOn && (admin ? clients.length > 0 : clients.length > 1));
	const current = $derived(clients.find((client) => client.id === auth.currentClient) ?? null);
	const value = $derived(auth.currentClient === null ? '' : String(auth.currentClient));

	function pick(raw: string) {
		const next = raw === '' ? null : Number(raw);
		if (next === auth.currentClient) return;
		auth.selectClient(next);
	}
</script>

{#if shown}
	{#if wide}
		<!-- Phone strip: the whole select, with its names in full. -->
		<div class="relative flex w-full items-center">
			<label for={id} class="sr-only">{m.msp_switcher_label()}</label>
			<Building2 class="pointer-events-none absolute left-2.5 size-4 text-ink-2" aria-hidden="true" />
			<select
				{id}
				class="input h-10 w-full min-w-0 truncate !pl-8 text-sm font-semibold"
				{value}
				title={m.msp_switcher_title()}
				onchange={(event) => pick(event.currentTarget.value)}
			>
				{#if admin}
					<option value="">{m.msp_switcher_all()}</option>
				{/if}
				{#each clients as client (client.id)}
					<option value={String(client.id)}>{client.name}</option>
				{/each}
			</select>
		</div>
	{:else}
		<!-- Top bar: an icon alone for "all clients", the (truncated) name once one is picked. The native select sits invisible over it. -->
		<div
			class={`relative flex h-10 min-w-10 max-w-[10rem] items-center justify-center gap-1.5 rounded-lg border border-line bg-surface text-ink-2 transition-colors duration-150 focus-within:border-line-strong hover:text-ink ${current ? 'px-2.5' : ''}`}
		>
			<Building2 class="pointer-events-none size-4 shrink-0" aria-hidden="true" />
			{#if current}
				<span class="pointer-events-none min-w-0 truncate text-sm font-semibold text-ink" aria-hidden="true">{current.name}</span>
			{/if}
			<label for={id} class="sr-only">{m.msp_switcher_label()}</label>
			<select
				{id}
				class="absolute inset-0 size-full cursor-pointer opacity-0"
				{value}
				title={current ? current.name : m.msp_switcher_title()}
				onchange={(event) => pick(event.currentTarget.value)}
			>
				{#if admin}
					<option value="">{m.msp_switcher_all()}</option>
				{/if}
				{#each clients as client (client.id)}
					<option value={String(client.id)}>{client.name}</option>
				{/each}
			</select>
		</div>
	{/if}
{/if}
