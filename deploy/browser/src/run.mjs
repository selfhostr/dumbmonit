// Exécution d'un scénario : contexte neuf, filtre réseau, interpréteur d'étapes, capture d'écran.
import { buildDenyList, checkHost, hostOf, hostResolverRules } from './guard.mjs';

const SHOT_MAX_BYTES = 512 * 1024;
const BLOCKED_MAX = 50;
const CLOSE_TIMEOUT_MS = 10_000;

const BASE_ARGS = [
  '--disable-dev-shm-usage',
  // WebRTC ne doit pas contourner le filtre réseau (fuite d'adresses, UDP direct).
  '--force-webrtc-ip-handling-policy=disable_non_proxied_udp',
  '--webrtc-ip-handling-policy=disable_non_proxied_udp',
  '--disable-features=WebRtcHideLocalIpsWithMdns',
];

export function launchOptions(extraArgs = [], sandbox = false) {
  // Sans bac à sable : le conteneur est durci (lecture seule, aucune capability).
  return { headless: true, chromiumSandbox: sandbox, args: [...BASE_ARGS, ...extraArgs] };
}

class StepError extends Error {
  constructor(kind, message, extra = {}) {
    super(message);
    this.kind = kind;
    this.extra = extra;
  }
}

// Remplace les valeurs des étapes secret:true (brutes et encodées pour une URL) par ***.
export function makeMasker(steps) {
  const vals = new Set();
  for (const s of steps) {
    if (s.secret && s.value) {
      vals.add(s.value);
      vals.add(encodeURIComponent(s.value));
    }
  }
  const sorted = [...vals].sort((a, b) => b.length - a.length);
  return (text) => (typeof text === 'string' ? sorted.reduce((acc, v) => acc.split(v).join('***'), text) : text);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const withTimeout = (p, ms) => Promise.race([p, sleep(ms)]);

const firstLine = (e) => String(e?.message ?? e).split('\n')[0].slice(0, 300);

// Interroge `probe` jusqu'à ce qu'elle renvoie true ou que le délai soit écoulé.
async function poll(probe, timeoutMs) {
  const end = Date.now() + timeoutMs;
  for (;;) {
    try {
      if (await probe()) return true;
    } catch {
      // navigation en cours : on réessaie
    }
    if (Date.now() >= end) return false;
    await sleep(Math.min(250, Math.max(0, end - Date.now())));
  }
}

async function doStep(page, step, timeout) {
  const loc = (sel) => page.locator(sel).first();
  switch (step.action) {
    case 'goto': {
      let resp;
      try {
        resp = await page.goto(step.url, { timeout, waitUntil: 'load' });
      } catch (e) {
        const msg = String(e?.message ?? e);
        if (msg.includes('ERR_BLOCKED_BY_CLIENT')) throw new StepError('blocked', 'navigation blocked by the network filter');
        const code = /net::ERR_[A-Z_]+/.exec(msg);
        if (code) throw new StepError('navigation', code[0]);
        if (e?.name === 'TimeoutError') throw new StepError('navigation', `navigation timed out after ${timeout} ms`);
        throw new StepError('navigation', firstLine(e));
      }
      const status = resp ? resp.status() : null;
      if (status !== null && status >= 400) throw new StepError('navigation', `HTTP ${status}`, { status });
      return { status };
    }
    case 'click':
      await loc(step.selector).click({ timeout });
      return {};
    case 'fill':
      await loc(step.selector).fill(step.value, { timeout });
      return {};
    case 'press':
      if (step.selector) await loc(step.selector).press(step.key, { timeout });
      else await page.keyboard.press(step.key);
      return {};
    case 'select':
      await loc(step.selector).selectOption(step.value, { timeout });
      return {};
    case 'wait_for': {
      const target = step.selector !== undefined ? loc(step.selector) : page.getByText(step.text).first();
      await target.waitFor({ state: step.state, timeout });
      return {};
    }
    case 'assert_text': {
      const ok = await poll(async () => {
        const body = await loc(step.selector ?? 'body').innerText({ timeout: Math.min(2000, timeout) });
        return body.includes(step.text) !== Boolean(step.negate);
      }, timeout);
      if (!ok) throw new StepError('assertion', step.negate ? `text '${step.text}' is present` : `text '${step.text}' not found`);
      return {};
    }
    case 'assert_url': {
      const re = step.regex !== undefined ? new RegExp(step.regex) : null;
      const test = (u) => (step.equals !== undefined ? u === step.equals : step.contains !== undefined ? u.includes(step.contains) : re.test(u));
      let last = page.url();
      const ok = await poll(async () => test((last = page.url())), timeout);
      if (!ok) throw new StepError('assertion', `url '${last}' does not match`);
      return {};
    }
    case 'sleep':
      await sleep(Math.min(step.ms, timeout));
      return {};
    default:
      throw new StepError('invalid', 'unknown action');
  }
}

function classify(e, step, timeout, budgetLimited) {
  if (e instanceof StepError) return e;
  if (e?.name === 'TimeoutError') {
    if (budgetLimited) return new StepError('timeout', 'total time budget exceeded');
    const what = step.selector ?? step.text ?? '';
    return new StepError('assertion', `${step.action} timed out after ${timeout} ms${what ? ` on '${what}'` : ''}`);
  }
  return new StepError('assertion', firstLine(e));
}

async function screenshot(page, steps) {
  try {
    const masks = [page.locator('input[type=password]')];
    for (const s of steps) if (s.secret && s.selector) masks.push(page.locator(s.selector));
    for (const quality of [60, 30]) {
      const buf = await page.screenshot({
        type: 'jpeg',
        quality,
        mask: masks,
        maskColor: '#000000',
        timeout: 5000,
        animations: 'disabled',
        caret: 'hide',
      });
      if (buf.length <= SHOT_MAX_BYTES) return { mime: 'image/jpeg', base64: buf.toString('base64') };
    }
  } catch {
    // jamais de capture non masquée en repli
  }
  return null;
}

// req : requête validée (schema.mjs). deps : { chromium, shared, sandbox, lookup }.
export async function executeRun(req, deps) {
  const started = Date.now();
  const deadline = started + req.timeout_ms;
  const mask = makeMasker(req.steps);
  const net = req.network;
  const guard = { denyList: buildDenyList(net.deny_cidrs), allowPrivate: net.allow_private, allowHosts: new Set(net.allow_hosts) };
  const verdicts = new Map();
  const verdict = (host) => {
    if (!verdicts.has(host)) verdicts.set(host, checkHost(host, guard, deps.lookup));
    return verdicts.get(host);
  };

  // Hôtes des goto : validés ici puis figés dans Chromium (anti DNS rebinding du niveau supérieur).
  const mappings = new Map();
  if (!guard.allowPrivate) {
    for (const s of req.steps) {
      if (s.action !== 'goto') continue;
      const host = hostOf(s.url);
      const v = await verdict(host);
      if (v.ok && v.addrs.length > 0 && !mappings.has(host) && !guard.allowHosts.has(host)) {
        const first = v.addrs.find((a) => !a.includes(':')) ?? v.addrs[0];
        mappings.set(host, first);
      }
    }
  }
  const rules = hostResolverRules(mappings);
  let browser = deps.shared;
  let ownBrowser = false;
  if (rules) {
    browser = await deps.chromium.launch(launchOptions([`--host-resolver-rules=${rules}`], deps.sandbox));
    ownBrowser = true;
  }

  const blocked = [];
  const block = async (route, url, reason) => {
    if (blocked.length < BLOCKED_MAX) blocked.push({ url: mask(String(url).slice(0, 512)), reason });
    await route.abort('blockedbyclient').catch(() => {});
  };

  let context;
  try {
    context = await browser.newContext({
      acceptDownloads: false,
      serviceWorkers: 'block',
      ignoreHTTPSErrors: req.ignore_https_errors,
      viewport: req.viewport,
      userAgent: req.user_agent,
      permissions: [],
    });
    await context.route('**/*', async (route, request) => {
      try {
        const url = request.url();
        let proto;
        try {
          proto = new URL(url).protocol;
        } catch {
          return await block(route, url, 'scheme');
        }
        if (proto === 'http:' || proto === 'https:') {
          const v = await verdict(hostOf(url));
          if (!v.ok) return await block(route, url, v.reason);
          return await route.continue();
        }
        const nav = request.isNavigationRequest();
        if ((proto === 'data:' || proto === 'blob:') && !nav) return await route.continue();
        if (proto === 'about:' && url === 'about:blank') return await route.continue();
        return await block(route, url, nav ? 'navigation_scheme' : 'scheme');
      } catch {
        await route.abort('failed').catch(() => {});
      }
    });
    await context.routeWebSocket(/.*/, async (ws) => {
      try {
        const v = await verdict(hostOf(ws.url()));
        if (v.ok) ws.connectToServer();
        else {
          if (blocked.length < BLOCKED_MAX) blocked.push({ url: mask(ws.url().slice(0, 512)), reason: v.reason });
          await ws.close();
        }
      } catch {
        await ws.close().catch(() => {});
      }
    });

    const page = await context.newPage();
    page.on('dialog', (d) => d.dismiss().catch(() => {}));
    page.on('download', (d) => d.cancel().catch(() => {}));

    const results = [];
    let failed = null;
    for (let i = 0; i < req.steps.length; i++) {
      const step = req.steps[i];
      const index = i + 1;
      const base = { index, action: step.action };
      if (step.name) base.name = step.name;
      if (failed) {
        results.push({ ...base, ok: null, duration_ms: 0 });
        continue;
      }
      const remaining = deadline - Date.now();
      const t0 = Date.now();
      let err = null;
      let extra = {};
      let timeout = Math.min(req.step_timeout_ms, Math.max(remaining, 0));
      if (remaining <= 0) err = new StepError('timeout', 'total time budget exceeded');
      else {
        try {
          extra = await doStep(page, step, timeout);
        } catch (e) {
          err = classify(e, step, timeout, remaining < req.step_timeout_ms);
        }
      }
      const r = { ...base, ok: err === null, duration_ms: Date.now() - t0 };
      if (step.action === 'goto') {
        r.url = mask(page.url());
        if (extra.status !== undefined && extra.status !== null) r.status = extra.status;
      }
      if (err) {
        if (err.extra?.status !== undefined) r.status = err.extra.status;
        r.error = mask(err.message);
        failed = { index, kind: err.kind, message: r.error };
      }
      results.push(r);
    }

    const out = {
      ok: failed === null,
      duration_ms: Date.now() - started,
      failed_step: failed ? failed.index : null,
      error: failed ? { kind: failed.kind, message: failed.message } : null,
      steps: results,
      blocked_requests: blocked,
      final_url: mask(page.url()),
      screenshot: null,
    };
    if (req.screenshot === 'always' || (req.screenshot === 'on_failure' && failed)) {
      out.screenshot = await screenshot(page, req.steps);
    }
    return out;
  } finally {
    if (context) await withTimeout(context.close().catch(() => {}), CLOSE_TIMEOUT_MS);
    if (ownBrowser) await withTimeout(browser.close().catch(() => {}), CLOSE_TIMEOUT_MS);
  }
}
