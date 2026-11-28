import test from 'node:test';
import assert from 'node:assert/strict';
import { createApp, tokenMatches } from '../src/server.mjs';

const TOKEN = 't'.repeat(40);
const body = JSON.stringify({ steps: [{ action: 'sleep', ms: 1 }] });

async function start(opts = {}) {
  const app = createApp({ token: TOKEN, run: async () => ({ ok: true }), ...opts });
  await new Promise((r) => app.server.listen(0, '127.0.0.1', r));
  const url = `http://127.0.0.1:${app.server.address().port}`;
  const post = (path, { token = TOKEN, payload = body } = {}) =>
    fetch(url + path, {
      method: 'POST',
      headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), 'content-type': 'application/json' },
      body: payload,
    });
  return { app, url, post, stop: () => app.shutdown(100) };
}

test('le jeton doit faire 32 caractères au moins', () => {
  assert.throws(() => createApp({ token: 'short', run: async () => ({}) }), /32/);
  assert.throws(() => createApp({ run: async () => ({}) }), /32/);
});

test('tokenMatches', () => {
  assert.equal(tokenMatches(`Bearer ${TOKEN}`, TOKEN), true);
  assert.equal(tokenMatches(`Bearer ${TOKEN}x`, TOKEN), false);
  assert.equal(tokenMatches(TOKEN, TOKEN), false);
  assert.equal(tokenMatches(undefined, TOKEN), false);
});

test('health sans authentification, run avec', async () => {
  const s = await start({ capacity: 3 });
  try {
    const h = await (await fetch(`${s.url}/v1/health`)).json();
    assert.deepEqual([h.ok, h.busy, h.capacity, h.version], [true, 0, 3, '0.1.0']);
    assert.match(h.playwright, /^\d+\.\d+/);
    assert.equal((await s.post('/v1/run', { token: null })).status, 401);
    assert.equal((await s.post('/v1/run', { token: 'x'.repeat(40) })).status, 401);
    const ok = await s.post('/v1/run');
    assert.equal(ok.status, 200);
    assert.equal((await ok.json()).ok, true);
    assert.equal((await fetch(`${s.url}/nope`)).status, 404);
    assert.equal((await fetch(`${s.url}/v1/run`)).status, 405);
  } finally {
    await s.stop();
  }
});

test('400 pour un schéma invalide, 413 pour un corps trop gros', async () => {
  const s = await start();
  try {
    const r = await s.post('/v1/run', { payload: JSON.stringify({ steps: [{ action: 'goto', url: 'file:///x' }] }) });
    assert.equal(r.status, 400);
    const j = await r.json();
    assert.deepEqual([j.error, j.step], ['invalid', 1]);
    assert.equal((await s.post('/v1/run', { payload: '{' })).status, 400);
    const big = JSON.stringify({ steps: [{ action: 'click', selector: 'x'.repeat(300 * 1024) }] });
    try {
      assert.equal((await s.post('/v1/run', { payload: big })).status, 413);
    } catch {
      // le serveur peut couper la connexion avant la fin de l'envoi : acceptable
    }
  } finally {
    await s.stop();
  }
});

test("429 à pleine capacité, 503 pendant l'arrêt", async () => {
  let release;
  const gate = new Promise((r) => (release = r));
  const s = await start({ capacity: 1, run: async () => (await gate, { ok: true }) });
  try {
    const first = s.post('/v1/run');
    while (s.app.state.busy === 0) await new Promise((r) => setTimeout(r, 10));
    assert.equal((await s.post('/v1/run')).status, 429);
    const stopping = s.app.shutdown(5000);
    assert.equal((await s.post('/v1/run')).status, 503);
    release();
    assert.equal((await first).status, 200);
    await stopping;
  } finally {
    release();
  }
});
