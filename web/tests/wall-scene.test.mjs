// The wall's world: Paris by default, `none` as an explicit choice, retired backdrops folded onto Paris. Run: npm test
import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import {
	readWallScene,
	writeWallScene,
	parseForcedWallScene,
	resolveWallScene,
	DEFAULT_WALL_SCENE
} from '../src/lib/components/wall/wallScene.ts';

const store = new Map();
globalThis.localStorage = {
	getItem: (k) => (store.has(k) ? store.get(k) : null),
	setItem: (k, v) => void store.set(k, String(v)),
	removeItem: (k) => void store.delete(k)
};
beforeEach(() => store.clear());

test('nothing saved, or junk, is Paris', () => {
	assert.equal(DEFAULT_WALL_SCENE, 'paris');
	assert.equal(readWallScene(), 'paris');
	store.set('dumbmonit-wall-city', 'atlantis');
	assert.equal(readWallScene(), 'paris');
	assert.equal(resolveWallScene(readWallScene()), 'paris');
});

test('every retired backdrop, and auto, lands on Paris', () => {
	for (const old of ['auto', 'newyork', 'tokyo', 'dubai', 'zermatt', 'matterhorn', 'london', 'rome', 'hongkong', 'kyoto', 'rio']) {
		store.set('dumbmonit-wall-city', old);
		assert.equal(readWallScene('none'), 'paris', old);
		assert.equal(parseForcedWallScene(old), 'paris', old);
	}
});

test('choices round-trip, and a plain board is a stored choice', () => {
	writeWallScene('paris');
	assert.equal(readWallScene('none'), 'paris');
	writeWallScene('none');
	assert.equal(store.get('dumbmonit-wall-city'), 'none');
	assert.equal(readWallScene(), 'none');
	assert.equal(resolveWallScene(readWallScene()), null);
});

test('a ?city= value names a choice, junk names nothing', () => {
	assert.equal(parseForcedWallScene('none'), 'none');
	assert.equal(parseForcedWallScene('paris'), 'paris');
	assert.equal(parseForcedWallScene('atlantis'), null);
	assert.equal(parseForcedWallScene(null), null);
});
