// Community registry: search and official address. Run: npm test
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { OFFICIAL_REGISTRY_URL, filterRegistryPacks } from '../src/lib/components/settings/pack-registry.ts';

const pack = (id, o = {}) => ({ id, name: id, version: '1.0.0', description: '', author: '', license: 'MIT', status: 'available', ...o });
const packs = [
	pack('ups-apc', { name: 'APC UPS', description: 'Onduleurs réseau', author: 'Alice' }),
	pack('printer-hp', { name: 'HP printers', license: 'Apache-2.0' })
];

test('the official registry is an https index.json', () => {
	assert.match(OFFICIAL_REGISTRY_URL, /^https:\/\/.+\/index\.json$/);
});

test('an empty search keeps every pack', () => {
	assert.equal(filterRegistryPacks(packs, '  ').length, 2);
});

test('search ignores case and accents, and needs every word', () => {
	assert.deepEqual(filterRegistryPacks(packs, 'ONDULEURS reseau').map((p) => p.id), ['ups-apc']);
	assert.deepEqual(filterRegistryPacks(packs, 'apache').map((p) => p.id), ['printer-hp']);
	assert.equal(filterRegistryPacks(packs, 'apc hp').length, 0);
});
