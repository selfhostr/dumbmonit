import test from 'node:test';
import assert from 'node:assert/strict';
import { InvalidRequest, LIMITS, validateRun } from '../src/schema.mjs';

const ok = (extra = {}) => ({ steps: [{ action: 'goto', url: 'https://example.com/' }], ...extra });

test('applique les valeurs par défaut et borne les délais', () => {
  const r = validateRun(ok({ timeout_ms: 999_999, step_timeout_ms: 999_999 }));
  assert.equal(r.timeout_ms, LIMITS.maxTimeoutMs);
  assert.equal(r.step_timeout_ms, LIMITS.maxStepTimeoutMs);
  assert.deepEqual(r.viewport, { width: 1280, height: 800 });
  assert.equal(r.screenshot, 'on_failure');
  assert.equal(r.network.allow_private, false);
  assert.equal(validateRun(ok()).timeout_ms, 60_000);
});

test('accepte toutes les actions', () => {
  const r = validateRun({
    steps: [
      { action: 'goto', url: 'http://a.test/', name: 'open' },
      { action: 'click', selector: '#a' },
      { action: 'fill', selector: '#u', value: 'x', secret: true },
      { action: 'press', key: 'Enter' },
      { action: 'select', selector: 's', value: 'v' },
      { action: 'wait_for', text: 'Hi', state: 'hidden' },
      { action: 'assert_text', text: 'Hi', negate: true },
      { action: 'assert_url', regex: '^https://' },
      { action: 'sleep', ms: 10 },
    ],
  });
  assert.equal(r.steps.length, 9);
  assert.equal(r.steps[0].name, 'open');
});

const bad = (steps, step) => {
  assert.throws(
    () => validateRun({ steps }),
    (e) => e instanceof InvalidRequest && (step === undefined || e.step === step),
  );
};

test('refuse les étapes invalides avec leur numéro', () => {
  bad([{ action: 'goto', url: 'https://ok.test/' }, { action: 'evaluate', script: '1' }], 2);
  bad([{ action: 'goto', url: 'file:///etc/passwd' }], 1);
  bad([{ action: 'goto', url: 'https://u:p@a.test/' }], 1);
  bad([{ action: 'goto', url: 'ftp://a.test/' }], 1);
  bad([{ action: 'click' }], 1);
  bad([{ action: 'click', selector: '#a', extra: 1 }], 1);
  bad([{ action: 'wait_for' }], 1);
  bad([{ action: 'wait_for', text: 'a', selector: 'b' }], 1);
  bad([{ action: 'wait_for', text: 'a', state: 'attached' }], 1);
  bad([{ action: 'assert_url', equals: 'a', contains: 'b' }], 1);
  bad([{ action: 'assert_url' }], 1);
  bad([{ action: 'assert_url', regex: '(' }], 1);
  bad([{ action: 'assert_url', regex: 'a'.repeat(513) }], 1);
  bad([{ action: 'sleep', ms: 10_001 }], 1);
  bad([{ action: 'sleep', ms: -1 }], 1);
  bad([{ action: 'fill', selector: 'a', value: 'x'.repeat(2049) }], 1);
  bad([{ action: 'fill', selector: 'a', value: 1 }], 1);
  bad(['goto'], 1);
});

test('refuse les requêtes mal formées', () => {
  assert.throws(() => validateRun(null), InvalidRequest);
  assert.throws(() => validateRun({ steps: [] }), InvalidRequest);
  assert.throws(() => validateRun({ steps: Array(31).fill({ action: 'sleep', ms: 1 }) }), InvalidRequest);
  assert.throws(() => validateRun(ok({ screenshot: 'sometimes' })), InvalidRequest);
  assert.throws(() => validateRun(ok({ user_agent: 'a\nb' })), InvalidRequest);
  assert.throws(() => validateRun(ok({ network: { deny_cidrs: 'x' } })), InvalidRequest);
  assert.equal(validateRun({ steps: Array(30).fill({ action: 'sleep', ms: 1 }) }).steps.length, 30);
});
