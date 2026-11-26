<script lang="ts">
	/**
	 * Settings → My account → Appearance: the theme and the language, both
	 * kept in this browser. The console is deep black by default; light is an
	 * option, and "System" follows the device. The wall stays dark.
	 *	 */
	import { Moon, Sun, SunMoon } from 'lucide-svelte';
	import { Panel, Toggle } from '#lib/ui/index.js';
	import Segmented from '#lib/ui/Segmented.svelte';
	import { displayPrefs } from '#lib/stores/prefs.svelte.js';
	import { m } from '#lib/paraglide/messages.js';
	import { theme, type ConsoleTheme } from '#lib/stores/theme.svelte.js';
	import LanguageSelect from '#lib/components/LanguageSelect.svelte';

	const CHOICES: { value: ConsoleTheme; label: () => string; icon: typeof Moon }[] = [
		{ value: 'dark', label: () => m.settings_appearance_theme_dark(), icon: Moon },
		{ value: 'light', label: () => m.settings_appearance_theme_light(), icon: Sun },
		{ value: 'auto', label: () => m.settings_appearance_theme_system(), icon: SunMoon }
	];

	const TIME_OPTIONS = $derived([
		{ value: 'auto', label: m.w2global_prefs_auto() },
		{ value: '12', label: m.w2global_prefs_time_12() },
		{ value: '24', label: m.w2global_prefs_time_24() }
	]);
	const WEEK_OPTIONS = $derived([
		{ value: 'auto', label: m.w2global_prefs_auto() },
		{ value: 'monday', label: m.w2global_prefs_week_monday() },
		{ value: 'sunday', label: m.w2global_prefs_week_sunday() }
	]);
	const DENSITY_OPTIONS = $derived([
		{ value: 'comfortable', label: m.w2global_prefs_density_comfortable() },
		{ value: 'compact', label: m.w2global_prefs_density_compact() }
	]);
	const MOTION_OPTIONS = $derived([
		{ value: 'auto', label: m.w2global_prefs_motion_system() },
		{ value: 'reduce', label: m.w2global_prefs_motion_reduce() }
	]);
</script>

<Panel id="appearance" title={m['settings.section.appearance']()} description={m.settings_language_theme_description()}>
	<div class="grid gap-4">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<span id="settings-theme-label" class="text-sm font-semibold text-ink">{m.settings_appearance_theme_label()}</span>
			<div role="radiogroup" aria-labelledby="settings-theme-label" class="flex rounded-lg border border-line-strong bg-surface-2 p-0.5 max-sm:w-full">
				{#each CHOICES as choice (choice.value)}
					{@const checked = theme.consoleChoice === choice.value}
					<label
						class={`relative flex h-10 min-w-0 flex-1 cursor-pointer items-center justify-center gap-1.5 rounded-md px-3 text-sm font-medium transition-colors duration-150 has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-offset-1 has-[:focus-visible]:outline-focus sm:min-w-24 ${checked ? 'bg-surface text-ink shadow-lift' : 'text-ink-2 hover:text-ink'}`}
					>
						<input
							type="radio"
							name="console-theme"
							value={choice.value}
							{checked}
							class="sr-only"
							onchange={() => theme.setConsole(choice.value)}
						/>
						<choice.icon class="size-4 shrink-0" strokeWidth={1.75} aria-hidden="true" />
						<span class="truncate">{choice.label()}</span>
					</label>
				{/each}
			</div>
		</div>
		<div class="flex flex-wrap items-center justify-between gap-3 max-sm:flex-nowrap">
			<label for="settings-language" class="text-sm font-semibold text-ink">{m['settings.appearance.language_label']()}</label>
			<LanguageSelect id="settings-language" class="w-full max-sm:w-auto max-sm:min-w-0 max-sm:flex-1 sm:w-auto sm:min-w-56" />
		</div>
		<div class="grid gap-4 border-t border-line pt-4 sm:grid-cols-2">
			<Segmented
				legend={m.w2global_prefs_time_label()}
				name="pref-time-format"
				options={TIME_OPTIONS}
				value={displayPrefs.timeFormat}
				onchange={(v) => displayPrefs.set({ timeFormat: v as 'auto' | '12' | '24' })}
			/>
			<Segmented
				legend={m.w2global_prefs_week_label()}
				name="pref-week-start"
				options={WEEK_OPTIONS}
				value={displayPrefs.weekStart}
				onchange={(v) => displayPrefs.set({ weekStart: v as 'auto' | 'monday' | 'sunday' })}
			/>
			<Segmented
				legend={m.w2global_prefs_density_label()}
				name="pref-density"
				options={DENSITY_OPTIONS}
				value={displayPrefs.density}
				onchange={(v) => displayPrefs.set({ density: v as 'comfortable' | 'compact' })}
			/>
			<Segmented
				legend={m.w2global_prefs_motion_label()}
				name="pref-motion"
				options={MOTION_OPTIONS}
				value={displayPrefs.motion}
				help={m.w2global_prefs_motion_help()}
				onchange={(v) => displayPrefs.set({ motion: v as 'auto' | 'reduce' })}
			/>
		</div>
		<div class="grid gap-3 border-t border-line pt-4">
			<div class="flex items-center gap-3">
				<Toggle id="pref-show-github" checked={displayPrefs.showGithub} label={m.settings_appearance_show_github()} onchange={(on) => displayPrefs.set({ showGithub: on })} />
				<label for="pref-show-github" class="text-sm font-semibold text-ink">{m.settings_appearance_show_github()}</label>
			</div>
			<div class="flex items-center gap-3">
				<Toggle id="pref-show-docs" checked={displayPrefs.showDocs} label={m.settings_appearance_show_docs()} onchange={(on) => displayPrefs.set({ showDocs: on })} />
				<label for="pref-show-docs" class="text-sm font-semibold text-ink">{m.settings_appearance_show_docs()}</label>
			</div>
			<p class="m-0 text-[0.8125rem] text-ink-3">{m.settings_appearance_links_help()}</p>
		</div>
	</div>
</Panel>
