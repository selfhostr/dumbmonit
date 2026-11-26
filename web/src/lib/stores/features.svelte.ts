/**
 * Optional modules (`GET /api/settings/features`): wall, SLA reports, status
 * pages, SLO and the threat map. Off on a new install; the navigation, the
 * settings sections and the pages of a module that is off are hidden.
 *
 * While the answer is not in, every module counts as off (nothing flashes).
 * If it cannot be read (an older server, a wall account the server does not
 * let through), every module counts as on: the server is the one that refuses.
 */
import { getFeatures, updateFeatures } from '#lib/api/features.js';
import type { Features, FeaturesPatch } from '#lib/api/types.js';

const KEYS: (keyof Features)[] = ['wall', 'sla_reports', 'status_pages', 'slo', 'threat_map'];

/** Anything that is not a full object of booleans is not an answer. */
function parse(value: unknown): Features | null {
	if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
	const record = value as Record<string, unknown>;
	if (!KEYS.every((key) => typeof record[key] === 'boolean')) return null;
	return value as Features;
}

class FeaturesStore {
	/** `null` until read, or when it could not be. */
	data = $state<Features | null>(null);
	/** True once the server answered (or failed to). */
	loaded = $state(false);
	/** The answer could not be read: the modules count as on. */
	unknown = $state(false);
	#loading: Promise<void> | null = null;

	/** Reads the modules once a session exists; later calls reuse the first read. */
	load(): Promise<void> {
		if (this.loaded) return Promise.resolve();
		this.#loading ??= (async () => {
			try {
				const data = parse(await getFeatures());
				this.data = data;
				this.unknown = data === null;
			} catch {
				this.unknown = true;
			} finally {
				this.loaded = true;
				this.#loading = null;
			}
		})();
		return this.#loading;
	}

	/** Whether a module is on. */
	on(key: keyof Features): boolean {
		if (this.data) return this.data[key];
		return this.loaded && this.unknown;
	}

	/** Administrators: changes some modules; the server returns the whole object. */
	async update(patch: FeaturesPatch): Promise<Features> {
		const data = await updateFeatures(patch);
		this.data = data;
		this.unknown = false;
		this.loaded = true;
		return data;
	}
}

export const featuresStore = new FeaturesStore();
