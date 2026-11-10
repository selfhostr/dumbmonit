// Public status page: day tones and states (tooltip words) of the history bar. Run: npm test
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dayTone, dayState, DAY_GOOD, DAY_POOR, DAY_MAJOR } from '../src/lib/components/status/day-tone.ts';

test('a day without data is grey, never green', () => {
	assert.equal(dayTone(null), 'ghost');
});

test('days are toned by their uptime band', () => {
	assert.equal(dayTone(100), 'signal');
	assert.equal(dayTone(DAY_GOOD), 'signal');
	assert.equal(dayTone(99.02), 'advisory');
	assert.equal(dayTone(96.62), 'advisory');
	assert.equal(dayTone(DAY_POOR), 'advisory');
	assert.equal(dayTone(92.97), 'warning');
	assert.equal(dayTone(0), 'warning');
});

test('a few minutes of downtime keep a day out of the poor band', () => {
	// 98 % = about 29 minutes down: some downtime, not a poor day.
	assert.equal(dayTone(98), 'advisory');
	assert.ok(DAY_POOR < DAY_GOOD);
});

test('each day has a state named in words, never colour alone', () => {
	assert.equal(dayState(null), 'nodata');
	assert.equal(dayState(100), 'operational');
	assert.equal(dayState(DAY_GOOD), 'operational');
	assert.equal(dayState(98), 'degraded');
	assert.equal(dayState(DAY_POOR), 'degraded');
	assert.equal(dayState(80), 'partial');
	assert.equal(dayState(DAY_MAJOR), 'partial');
	assert.equal(dayState(10), 'major');
	assert.equal(dayState(0), 'major');
});
