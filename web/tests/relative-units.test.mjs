import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const load = (locale) => JSON.parse(readFileSync(new URL(`../messages/${locale}.json`, import.meta.url), 'utf8'));
const UNITS = ['misc_dur_s', 'misc_dur_min', 'misc_dur_h', 'misc_dur_d'];

test('English and Chinese glue the number to the unit ("3d ago", "3天前")', () => {
	for (const locale of ['en', 'zh-Hans']) {
		const messages = load(locale);
		for (const key of UNITS) assert.doesNotMatch(messages[key], /\s/, `${locale}.${key}`);
	}
});

test('locales that space the unit use a non-breaking space, so "3 j" never wraps', () => {
	for (const locale of ['fr', 'de', 'es', 'ru']) {
		const messages = load(locale);
		for (const key of UNITS) {
			assert.match(messages[key], /^\{n\} \S+$/, `${locale}.${key}`);
		}
	}
});

test('no locale keeps the counters removed from the Overview', () => {
	for (const locale of ['en', 'fr', 'de', 'es', 'it', 'pt', 'pt-BR', 'ru', 'zh-Hans']) {
		const messages = load(locale);
		for (const key of ['overview_sky_n_suppressed', 'overview_sky_n_reporting', 'overview_sky_n_waiting']) {
			assert.equal(key in messages, false, `${locale}.${key}`);
		}
	}
});
