import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { browserSpeaksOurs, fallbackLocale } from '../src/lib/components/status/public-locale.ts';

const read = (path) => JSON.parse(readFileSync(new URL(path, import.meta.url), 'utf8'));
const uk = read('../messages/uk.json');
const en = read('../messages/en.json');
const { locales } = read('../project.inlang/settings.json');

test('Ukrainian is available to regional browsers on public status pages', () => {
	assert.ok(locales.includes('uk'));
	assert.equal(browserSpeaksOurs(['uk-UA'], locales), true);
	assert.equal(fallbackLocale({
		stored: null, languages: ['uk-UA'], locales, current: 'uk', defaultLang: 'fr'
	}), null);
});

test('Ukrainian messages cover every category used by its plural rules', () => {
	const categories = new Intl.PluralRules('uk').resolvedOptions().pluralCategories;
	for (const [key, value] of Object.entries(uk)) {
		if (!Array.isArray(value)) continue;
		for (const rule of value) {
			for (const selector of rule.selectors) {
				for (const category of categories) {
					assert.equal(typeof rule.match[selector + '=' + category], 'string', key + ': ' + category);
				}
			}
		}
	}
	const match = uk.worldmap_monitors[0].match;
	const rules = new Intl.PluralRules('uk');
	for (const [count, expected] of [
		[1, '1 об’єкт моніторингу'], [2, '2 об’єкти моніторингу'],
		[5, '5 об’єктів моніторингу'], [11, '11 об’єктів моніторингу'],
		[21, '21 об’єкт моніторингу'], [22, '22 об’єкти моніторингу'],
		[25, '25 об’єктів моніторингу']
	]) {
		assert.equal(match['countPlural=' + rules.select(count)].replace('{count}', count), expected);
	}
});

test('Ukrainian copy keeps configuration identifiers and the CSV import example usable', () => {
	for (const [key, value] of Object.entries(en)) {
		if (typeof value !== 'string') continue;
		for (const token of value.match(/\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b/g) ?? []) {
			assert.ok(uk[key].includes(token), key + ': ' + token);
		}
	}
	assert.equal(uk.devices_import_paste_placeholder, en.devices_import_paste_placeholder);
	assert.equal(uk['settings.appearance.language_label'], 'Мова');
});
