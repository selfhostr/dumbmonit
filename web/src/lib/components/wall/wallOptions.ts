/**
 * The labels of the wall's choices, shared by its Backdrop button
 * and by Settings. Kept apart from `wallScene.ts` (pure, loaded by the tests)
 * and from the scenes themselves (so Settings never carries any drawing).
 */
import { m } from '#lib/paraglide/messages.js';
import type { WallSceneChoice } from '#lib/components/wall/wallScene.js';

/** Paris, then a plain board. */
export function wallSceneOptions(): { value: WallSceneChoice; label: string }[] {
	return [
		{ value: 'paris', label: m.wall_scene_paris() },
		{ value: 'none', label: m.wall_scene_none() }
	];
}
