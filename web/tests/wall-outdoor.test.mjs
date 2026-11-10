// The outside weather beside the wall's date: icons, degrees. Run: npm test
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { outdoorIcon, outdoorDegrees, OUTDOOR_REFRESH_MS } from '../src/lib/components/wall/outdoor.ts';

test('each condition has its icon, and the clear sky and the partial cloud turn to the moon at night', () => {
	assert.equal(outdoorIcon('clear', true), 'sun');
	assert.equal(outdoorIcon('clear', false), 'moon');
	assert.equal(outdoorIcon('partly_cloudy', true), 'cloud-sun');
	assert.equal(outdoorIcon('partly_cloudy', false), 'cloud-moon');
	assert.equal(outdoorIcon('cloudy', false), 'cloud');
	assert.equal(outdoorIcon('fog', true), 'cloud-fog');
	assert.equal(outdoorIcon('rain', true), 'cloud-rain');
	assert.equal(outdoorIcon('snow', false), 'cloud-snow');
	assert.equal(outdoorIcon('thunder', true), 'cloud-lightning');
});

test('the temperature is a whole number of degrees, minus sign included', () => {
	assert.equal(outdoorDegrees(12), '12°');
	assert.equal(outdoorDegrees(0), '0°');
	assert.equal(outdoorDegrees(-0.2), '0°');
	assert.equal(outdoorDegrees(-4), '−4°');
	assert.equal(outdoorDegrees(11.6), '12°');
});

test('the wall asks no more often than the server refreshes', () => {
	assert.ok(OUTDOOR_REFRESH_MS >= 10 * 60_000);
});
