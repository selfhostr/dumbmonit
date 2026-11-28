import { m } from '#lib/paraglide/messages.js';
import type { BandFigure } from '#lib/components/wall/WallBand.svelte';

export interface BandInput {
	up: number;
	total: number;
	down: number;
	paused: number;
	/** Mean availability over 30 days (percent), null when unknown. */
	availability: number | null;
	/** How many monitors the availability averages. */
	availabilityCount: number;
	/** Daily availability (percent, null = gap), oldest first. */
	availabilitySeries: (number | null)[];
	/** Alerts opened per day, oldest first (14 days), null while loading. */
	opened: number[] | null;
}

/** The three figures of the wall's bottom band. */
export function buildBandFigures(input: BandInput): BandFigure[] {
	const { up, total, down, paused, availability, opened } = input;
	const thisWeek = opened ? opened.slice(-7).reduce((a, b) => a + b, 0) : null;
	const lastWeek = opened ? opened.slice(0, -7).slice(-7).reduce((a, b) => a + b, 0) : 0;
	return [
		{
			id: 'up',
			label: m.console_kpi_up(),
			value: String(up),
			unit: m.console_kpi_up_unit({ total }),
			note: m.console_kpi_up_note({ down, paused }),
			tone: down > 0 ? 'text-warning-ink' : 'text-ink-2'
		},
		{
			id: 'availability',
			label: m.console_kpi_availability(),
			value: availability === null ? '—' : availability >= 99.9995 ? '100' : availability.toFixed(3),
			unit: availability === null ? '' : '%',
			note: availability === null ? m.console_kpi_none() : m.console_kpi_availability_note({ count: input.availabilityCount }),
			tone: 'text-ink-2',
			series: input.availabilitySeries,
			seriesMin: 99,
			seriesMax: 100
		},
		{
			id: 'opened',
			label: m.wall_band_opened(),
			value: thisWeek === null ? '—' : String(thisWeek),
			unit: '',
			note: thisWeek === null ? m.console_kpi_none() : m.wall_band_opened_note({ count: lastWeek }),
			tone: 'text-ink-2',
			series: opened ?? []
		}
	];
}
