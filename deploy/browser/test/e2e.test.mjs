// Scénarios réels contre une page locale. Ignorés seulement si le binaire Chromium est absent.
import test, { before, after } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import dns from 'node:dns/promises';
import { chromium } from 'playwright';
import { executeRun, launchOptions } from '../src/run.mjs';
import { validateRun } from '../src/schema.mjs';

const DENY = ['127.0.0.0/8', '169.254.0.0/16', '::1/128', 'fe80::/10', 'fd00:ec2::254/128', '0.0.0.0/8'];
const PAGES = {
  '/login': '<html><body><form action="/home" method="get"><input id="u" name="user"><input id="p" name="pass" type="password"><button type="submit">Go</button></form></body></html>',
  '/home': '<html><body><h1>Dashboard</h1></body></html>',
  '/metadata': '<html><body><img src="http://169.254.169.254/latest/x.png"><p>page</p></body></html>',
};

let browser = null;
let skipReason = false;
let server;
let base;

before(async () => {
  try {
    browser = await chromium.launch(launchOptions());
  } catch (e) {
    skipReason = `Chromium unavailable: ${String(e.message).split('\n')[0]}`;
    return;
  }
  server = http.createServer((req, res) => {
    const path = req.url.split('?')[0];
    if (PAGES[path]) {
      res.writeHead(200, { 'content-type': 'text/html' });
      res.end(PAGES[path]);
    } else {
      res.writeHead(404, { 'content-type': 'text/html' });
      res.end('<html><body>missing</body></html>');
    }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  base = `http://127.0.0.1:${server.address().port}`;
});

after(async () => {
  await browser?.close();
  await new Promise((r) => (server ? server.close(r) : r()));
});

// Ignoré seulement si Chromium n'a pas pu démarrer (binaire absent), jamais en dur.
const e2e = (name, fn) =>
  test(name, async (t) => {
    if (skipReason) return t.skip(skipReason);
    await fn();
  });

const run = (body, network = {}) =>
  executeRun(
    validateRun({ step_timeout_ms: 1500, timeout_ms: 20_000, network: { deny_cidrs: DENY, allow_hosts: ['127.0.0.1'], ...network }, ...body }),
    { chromium, shared: browser, sandbox: false, lookup: dns.lookup },
  );

e2e('scénario de connexion réussi, secrets masqués', async () => {
  const out = await run({
    screenshot: 'always',
    steps: [
      { action: 'goto', url: `${base}/login` },
      { action: 'fill', selector: '#u', value: 'alice' },
      { action: 'fill', selector: '#p', value: 's3cr3t', secret: true },
      { action: 'click', selector: 'button[type=submit]' },
      { action: 'wait_for', text: 'Dashboard' },
      { action: 'assert_text', text: 'Dashboard', selector: 'h1' },
      { action: 'assert_url', contains: '/home' },
    ],
  });
  assert.equal(out.ok, true, JSON.stringify(out.steps));
  assert.equal(out.failed_step, null);
  assert.equal(out.steps.length, 7);
  assert.equal(out.steps[0].status, 200);
  assert.match(out.final_url, /pass=\*\*\*/);
  assert.ok(!JSON.stringify(out).replace(out.screenshot.base64, '').includes('s3cr3t'));
  assert.equal(out.screenshot.mime, 'image/jpeg');
  assert.ok(Buffer.from(out.screenshot.base64, 'base64').length < 512 * 1024);
});

e2e('assertion en échec : étape, ignorées, capture', async () => {
  const out = await run({
    steps: [
      { action: 'goto', url: `${base}/home` },
      { action: 'fill', selector: '#nope', value: 'topsecret', secret: true },
      { action: 'assert_text', text: 'Dashboard' },
    ],
  });
  assert.equal(out.ok, false);
  assert.equal(out.failed_step, 2);
  assert.equal(out.error.kind, 'assertion');
  assert.equal(out.steps[2].ok, null);
  assert.ok(out.screenshot);
  assert.ok(!JSON.stringify(out.error).includes('topsecret'));

  const text = await run({ steps: [{ action: 'goto', url: `${base}/home` }, { action: 'assert_text', text: 'Dashboard', negate: true }] });
  assert.equal(text.failed_step, 2);
  assert.equal(text.screenshot !== null, true);
});

e2e('page 404 : navigation', async () => {
  const out = await run({ steps: [{ action: 'goto', url: `${base}/missing` }] });
  assert.equal(out.error.kind, 'navigation');
  assert.equal(out.steps[0].status, 404);
});

e2e('adresse privée refusée sans allow_hosts : blocked', async () => {
  const out = await run({ steps: [{ action: 'goto', url: `${base}/home` }] }, { allow_hosts: [] });
  assert.equal(out.ok, false);
  assert.equal(out.error.kind, 'blocked');
  assert.equal(out.blocked_requests[0].reason, 'deny_cidr');
});

e2e('allow_private laisse passer', async () => {
  const out = await run({ steps: [{ action: 'goto', url: `${base}/home` }] }, { allow_hosts: [], allow_private: true });
  assert.equal(out.ok, true);
});

e2e('sous-ressource vers les métadonnées cloud bloquée', async () => {
  const out = await run({ steps: [{ action: 'goto', url: `${base}/metadata` }, { action: 'assert_text', text: 'page' }] });
  assert.equal(out.ok, true);
  assert.ok(out.blocked_requests.some((b) => b.url.startsWith('http://169.254.169.254/') && b.reason === 'deny_cidr'));
});

e2e('budget total dépassé : kind timeout', async () => {
  const out = await run({ timeout_ms: 1000, steps: [{ action: 'sleep', ms: 1500 }, { action: 'sleep', ms: 10 }, { action: 'sleep', ms: 10 }] });
  assert.equal(out.ok, false);
  assert.equal(out.error.kind, 'timeout');
});
