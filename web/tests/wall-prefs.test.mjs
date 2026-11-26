// The wall's remembered options, its address parameters and its page rotation. Run: npm test
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseWallPrefs, parseWallParams, clockText, nextPage, DEFAULT_WALL_PREFS } from '../src/lib/components/wall/wallPrefs.ts';

test('nothing stored, or junk, gives the defaults', () => {
	assert.deepEqual(parseWallPrefs(null), DEFAULT_WALL_PREFS);
	assert.deepEqual(parseWallPrefs('{nope'), DEFAULT_WALL_PREFS);
	assert.equal(parseWallPrefs('{"rotateSeconds":7}').rotateSeconds, 0);
});

test('stored values come back', () => {
	const prefs = parseWallPrefs('{"clock":"12h","seconds":true,"criticalSound":true,"rotateSeconds":30}');
	assert.deepEqual(prefs, { clock: '12h', seconds: true, criticalSound: true, rotateSeconds: 30 });
});

test('the clock follows the format and the seconds', () => {
	const date = new Date(2026, 0, 5, 15, 4, 9);
	assert.equal(clockText(date, 'en', { clock: '24h', seconds: false }), '15:04');
	assert.equal(clockText(date, 'en', { clock: '24h', seconds: true }), '15:04:09');
	assert.match(clockText(date, 'en', { clock: '12h', seconds: false }), /^3:04\s?PM$/i);
});

test('address parameters', () => {
	const p = (q) => parseWallParams(new URLSearchParams(q));
	assert.deepEqual(p('theme=light&scene=none'), { theme: 'light', scene: 'none' });
	assert.deepEqual(p('theme=blue&scene=mars'), { theme: null, scene: null });
	assert.equal(p('scene=tokyo').scene, 'paris');
});

test('pages wrap around', () => {
	assert.equal(nextPage(0, 3), 1);
	assert.equal(nextPage(2, 3), 0);
	assert.equal(nextPage(0, 1), 0);
});

import { wallNotices } from '../src/lib/components/wall/wallNotice.ts';

test('the banner lists open incidents and maintenances in progress only', () => {
	const now = Date.parse('2026-01-05T12:00:00Z');
	const inc = (id, kind, status, starts_at, ends_at = null, severity = 'minor') => ({ id, kind, status, starts_at, ends_at, severity, title: `t${id}` });
	const list = wallNotices(
		[
			inc(1, 'incident', 'resolved', '2026-01-05 08:00:00'),
			inc(2, 'incident', 'investigating', '2026-01-05 09:00:00'),
			inc(3, 'maintenance', 'scheduled', '2026-01-06 09:00:00', '2026-01-06 10:00:00'),
			inc(4, 'maintenance', 'in_progress', '2026-01-05 11:00:00', '2026-01-05 13:00:00')
		],
		[{ id: 9, name: 'Backups', enabled: true, active_now: true, active_until: '2026-01-05 12:30:00' }],
		now
	);
	assert.deepEqual(list.map((n) => n.key), ['i2', 'i4', 's9']);
});
