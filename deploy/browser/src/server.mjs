// Sidecar HTTP (node:http) : GET /v1/health, POST /v1/run.
import http from 'node:http';
import crypto from 'node:crypto';
import dns from 'node:dns/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { InvalidRequest, validateRun } from './schema.mjs';
import { executeRun, launchOptions } from './run.mjs';

const MAX_BODY = 256 * 1024;
const require = createRequire(import.meta.url);
const VERSION = require('../package.json').version;

export function playwrightVersion() {
  try {
    return require('playwright/package.json').version;
  } catch {
    return 'unknown';
  }
}

function send(res, status, body, headers = {}) {
  const data = JSON.stringify(body);
  res.writeHead(status, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(data), 'cache-control': 'no-store', ...headers });
  res.end(data);
}

// Comparaison à temps constant : on compare des condensats de même longueur.
export function tokenMatches(header, token) {
  const m = /^Bearer (.+)$/.exec(header ?? '');
  if (!m) return false;
  const a = crypto.createHash('sha256').update(m[1]).digest();
  const b = crypto.createHash('sha256').update(token).digest();
  return crypto.timingSafeEqual(a, b);
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    req.on('data', (c) => {
      size += c.length;
      if (size > MAX_BODY) {
        reject(Object.assign(new Error('too large'), { code: 'TOO_LARGE' }));
        req.destroy();
        return;
      }
      chunks.push(c);
    });
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}

// Construit le serveur. `run(req)` exécute un scénario validé ; `isReady()` indique que Chromium répond.
export function createApp({ token, capacity = 2, run, isReady = () => true, playwright = playwrightVersion() }) {
  if (typeof token !== 'string' || token.length < 32) throw new Error('BROWSER_TOKEN must be at least 32 characters');
  const state = { busy: 0, shuttingDown: false, idle: [] };

  const server = http.createServer(async (req, res) => {
    try {
      const path = (req.url ?? '').split('?')[0];
      if (path === '/v1/health') {
        if (req.method !== 'GET') return send(res, 405, { error: 'method_not_allowed' });
        const ok = !state.shuttingDown && isReady();
        return send(res, ok ? 200 : 503, { ok, version: VERSION, playwright, busy: state.busy, capacity });
      }
      if (path !== '/v1/run') return send(res, 404, { error: 'not_found' });
      if (req.method !== 'POST') return send(res, 405, { error: 'method_not_allowed' });
      if (!tokenMatches(req.headers.authorization, token)) {
        return send(res, 401, { error: 'unauthorized' }, { 'www-authenticate': 'Bearer' });
      }
      if (state.shuttingDown) return send(res, 503, { error: 'shutting_down' });
      let parsed;
      try {
        parsed = validateRun(JSON.parse(await readBody(req)));
      } catch (e) {
        if (e?.code === 'TOO_LARGE') return send(res, 413, { error: 'too_large', message: 'body larger than 256 KiB' });
        if (e instanceof InvalidRequest) return send(res, 400, { error: 'invalid', message: e.message, step: e.step });
        if (e instanceof SyntaxError) return send(res, 400, { error: 'invalid', message: 'body is not valid JSON', step: null });
        throw e;
      }
      if (state.busy >= capacity) return send(res, 429, { error: 'busy', message: 'sidecar at capacity' }, { 'retry-after': '5' });
      state.busy++;
      try {
        send(res, 200, await run(parsed));
      } finally {
        state.busy--;
        if (state.busy === 0) for (const f of state.idle.splice(0)) f();
      }
    } catch (e) {
      console.error('run failed:', String(e?.message ?? e).split('\n')[0]);
      if (!res.headersSent) send(res, 500, { error: 'internal', message: 'internal error' });
      else res.destroy();
    }
  });
  server.requestTimeout = 150_000;
  server.headersTimeout = 10_000;

  // Arrêt propre : refuse les nouveaux runs (503), attend ceux en cours au plus `graceMs`.
  async function shutdown(graceMs) {
    state.shuttingDown = true;
    if (state.busy > 0) await Promise.race([new Promise((r) => state.idle.push(r)), new Promise((r) => setTimeout(r, graceMs).unref())]);
    await new Promise((r) => server.close(r));
    server.closeAllConnections?.();
  }
  return { server, state, shutdown };
}

async function main() {
  const { chromium } = await import('playwright');
  const token = process.env.BROWSER_TOKEN ?? '';
  if (token.length < 32) {
    console.error('BROWSER_TOKEN is required and must be at least 32 characters');
    process.exit(1);
  }
  const capacity = Math.min(16, Math.max(1, Number.parseInt(process.env.BROWSER_CONCURRENCY ?? '2', 10) || 2));
  const sandbox = process.env.BROWSER_SANDBOX === '1';
  const shared = await chromium.launch(launchOptions([], sandbox));
  let stopping = false;
  // Chromium disparu : on sort, la politique de redémarrage du conteneur prend le relais.
  shared.on('disconnected', () => {
    if (!stopping) {
      console.error('chromium disconnected, exiting');
      process.exit(1);
    }
  });
  const app = createApp({
    token,
    capacity,
    isReady: () => shared.isConnected(),
    run: (req) => executeRun(req, { chromium, shared, sandbox, lookup: dns.lookup }),
  });
  const port = Number.parseInt(process.env.BROWSER_PORT ?? '3000', 10);
  app.server.listen(port, '0.0.0.0', () => console.log(`dumbmonit-browser ${VERSION} (playwright ${playwrightVersion()}) on :${port}, capacity ${capacity}`));
  const stop = async () => {
    if (stopping) return;
    stopping = true;
    await app.shutdown(Number.parseInt(process.env.BROWSER_SHUTDOWN_GRACE_MS ?? '30000', 10));
    await shared.close().catch(() => {});
    process.exit(0);
  };
  process.on('SIGTERM', stop);
  process.on('SIGINT', stop);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((e) => {
    console.error('startup failed:', String(e?.message ?? e).split('\n')[0]);
    process.exit(1);
  });
}
