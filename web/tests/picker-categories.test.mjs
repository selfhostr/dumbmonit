// "Add a monitor": the categories and the filters of the picker. Run: npm test
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import {
	CATEGORY_IDS,
	CATEGORY_KINDS,
	categoryOf,
	needsLogin,
	passesFilters,
	rankInCategory
} from '../src/lib/components/device-form/categories.ts';

const choice = (o = {}) => ({ category: 'network', beta: false, pack: false, noLogin: false, ...o });

test('every kind of the catalogue snapshot has a category, except the ones made for tests', () => {
	const snapshot = JSON.parse(readFileSync(new URL('../../crates/server/tests/fixtures/collectors_catalog.json', import.meta.url), 'utf8'));
	const unplaced = snapshot.map((k) => k.kind).filter((kind) => categoryOf(kind) === 'other');
	assert.deepEqual(unplaced.sort(), ['agent_service', 'dummy', 'zz-inconnu']);
});

test('a kind sits in one category only, and every category is listed', () => {
	const ids = Object.values(CATEGORY_KINDS).flat();
	assert.equal(new Set(ids).size, ids.length);
	for (const category of Object.keys(CATEGORY_KINDS)) assert.ok(CATEGORY_IDS.includes(category), category);
});

test('packs and unknown kinds', () => {
	assert.equal(categoryOf('pack.printer'), 'packs');
	assert.equal(categoryOf('brand_new'), 'other');
	assert.equal(categoryOf('agent:relay'), 'machines');
	assert.ok(rankInCategory('snmp') < rankInCategory('proxmox') + 1000);
	assert.ok(rankInCategory('agent') < rankInCategory('agent:relay'));
	assert.equal(rankInCategory('brand_new'), Number.MAX_SAFE_INTEGER);
});

test('a kind needs a login unless it accepts "none" or declares nothing', () => {
	assert.equal(needsLogin(['snmp_community', 'snmp_v3']), true);
	assert.equal(needsLogin(['none']), false);
	assert.equal(needsLogin(['api_token', 'none']), false);
	assert.equal(needsLogin([]), false);
});

test('filters: categories are alternatives, facets all have to hold', () => {
	const none = new Set();
	assert.equal(passesFilters(choice(), none, none), true);
	assert.equal(passesFilters(choice(), new Set(['storage', 'network']), none), true);
	assert.equal(passesFilters(choice(), new Set(['storage']), none), false);
	const beta = choice({ beta: true });
	assert.equal(passesFilters(beta, none, new Set(['beta'])), true);
	assert.equal(passesFilters(choice(), none, new Set(['beta'])), false);
	assert.equal(passesFilters(beta, none, new Set(['beta', 'nologin'])), false);
	assert.equal(passesFilters(choice({ beta: true, noLogin: true }), new Set(['network']), new Set(['beta', 'nologin'])), true);
	assert.equal(passesFilters(choice({ pack: true, category: 'packs' }), none, new Set(['pack'])), true);
});
