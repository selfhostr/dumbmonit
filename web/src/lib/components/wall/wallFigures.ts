import { m } from '#lib/paraglide/messages.js';
import type { BandFigure } from '#lib/components/wall/WallBand.svelte';

export interface BandInput {
	up: number;
	total: number;
	/** Mean availability over 30 days (percent), null when unknown. */
	availability: number | null;
	/** Daily availability (percent, null = gap), oldest first. */
	availabilitySeries: (number | null)[];
}

/** The two figures of the wall's foot, under the problems: monitors up and availability. */
export function buildBandFigures(input: BandInput): BandFigure[] {
	const { up, total, availability } = input;
	return [
		{
			id: 'up',
			label: m.console_kpi_up(),
			value: String(up),
			unit: m.console_kpi_up_unit({ total })
		},
		{
			id: 'availability',
			label: m.console_kpi_availability(),
			value: availability === null ? '—' : availability >= 99.9995 ? '100' : availability.toFixed(3),
			unit: availability === null ? '' : '%',
			series: input.availabilitySeries,
			seriesMin: 99,
			seriesMax: 100
		}
	];
}
