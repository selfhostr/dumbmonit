// A wall-display account always lands on /wall. Run: npm test
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isWallPath, wallDestination } from '../src/lib/wall/route.ts';

test('on the wall, the account stays', () => {
	assert.equal(wallDestination('/wall'), null);
	assert.equal(wallDestination('/wall', '/targets'), null);
});

test('every other page sends it to the wall', () => {
	for (const path of ['/', '/targets', '/targets/3', '/alerts', '/settings', '/welcome', '/explore', '/wallpaper', '/login']) {
		assert.equal(wallDestination(path), '/wall', path);
	}
});

test('after sign-in, a remembered wall address keeps its options', () => {
	assert.equal(wallDestination('/login', '/wall?city=paris'), '/wall?city=paris');
	assert.equal(wallDestination('/login', '/wall'), '/wall');
});

test('any other remembered destination is ignored', () => {
	for (const redirect of ['/settings', '/', '//evil.example/wall', 'https://evil.example/wall', '/wall/../settings', '/wallet', '/wall\\x', '/wall?\u0000']) {
		assert.equal(wallDestination('/login', redirect), '/wall', redirect);
		assert.equal(isWallPath(redirect), false, redirect);
	}
});
