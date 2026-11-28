/**
 * Pure helpers of the community registry views (Settings and Add monitor).
 */
import type { RegistryPack } from '#lib/api/packs.js';

/** Address of the official registry, offered by one button while none is set. */
export const OFFICIAL_REGISTRY_URL = 'https://dumbmonit.selfhostr.io/hub/registry/index.json';

const fold = (text: string) =>
	text
		.normalize('NFD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase();

/** Packs whose name, id, description, author or licence contain every word of `query`. */
export function filterRegistryPacks(packs: RegistryPack[], query: string): RegistryPack[] {
	const words = fold(query).split(/\s+/).filter(Boolean);
	if (words.length === 0) return packs;
	return packs.filter((pack) => {
		const haystack = fold(`${pack.name} ${pack.id} ${pack.description} ${pack.author} ${pack.license}`);
		return words.every((word) => haystack.includes(word));
	});
}
