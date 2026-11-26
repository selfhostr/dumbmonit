// Display preferences model. Run: npm test
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parsePrefs, hour12Option, firstDayOverride, DEFAULT_PREFS } from '../src/lib/stores/prefs-model.ts';

test('defaults for nothing or garbage', () => {
	assert.deepEqual(parsePrefs(null), DEFAULT_PREFS);
	assert.deepEqual(parsePrefs('nope'), DEFAULT_PREFS);
	assert.deepEqual(parsePrefs('{"timeFormat":"7","density":"huge"}'), DEFAULT_PREFS);
});

test('valid choices are kept', () => {
	const p = parsePrefs('{"timeFormat":"12","weekStart":"sunday","density":"compact","motion":"reduce","showGithub":false,"showDocs":false}');
	assert.deepEqual(p, { timeFormat: '12', weekStart: 'sunday', density: 'compact', motion: 'reduce', showGithub: false, showDocs: false });
});

test('option mapping', () => {
	assert.equal(hour12Option('auto'), undefined);
	assert.equal(hour12Option('12'), true);
	assert.equal(hour12Option('24'), false);
	assert.equal(firstDayOverride('auto'), null);
	assert.equal(firstDayOverride('monday'), 1);
	assert.equal(firstDayOverride('sunday'), 0);
});
