// Validation et bornage de la requête POST /v1/run. Aucune dépendance.

export const LIMITS = {
  maxSteps: 30,
  maxString: 2048,
  maxRegex: 512,
  maxTimeoutMs: 120_000,
  maxStepTimeoutMs: 30_000,
  maxSleepMs: 10_000,
};

export class InvalidRequest extends Error {
  constructor(message, step) {
    super(message);
    this.step = step ?? null;
  }
}

// Champs autorisés par action (en plus de `action`, `name`).
const FIELDS = {
  goto: ['url'],
  click: ['selector'],
  fill: ['selector', 'value', 'secret'],
  press: ['selector', 'key'],
  select: ['selector', 'value', 'secret'],
  wait_for: ['selector', 'text', 'state'],
  assert_text: ['text', 'selector', 'negate'],
  assert_url: ['equals', 'contains', 'regex'],
  sleep: ['ms'],
};

const isObj = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);

function str(obj, key, step, { required = false, allowEmpty = false } = {}) {
  const v = obj[key];
  if (v === undefined || v === null) {
    if (required) throw new InvalidRequest(`missing field '${key}'`, step);
    return undefined;
  }
  if (typeof v !== 'string') throw new InvalidRequest(`'${key}' must be a string`, step);
  if (v.length > LIMITS.maxString) throw new InvalidRequest(`'${key}' is longer than ${LIMITS.maxString} characters`, step);
  if (!allowEmpty && v === '') throw new InvalidRequest(`'${key}' must not be empty`, step);
  return v;
}

function bool(obj, key, step) {
  const v = obj[key];
  if (v === undefined || v === null) return false;
  if (typeof v !== 'boolean') throw new InvalidRequest(`'${key}' must be a boolean`, step);
  return v;
}

function intIn(v, lo, hi, def, label) {
  if (v === undefined || v === null) return def;
  if (typeof v !== 'number' || !Number.isFinite(v)) throw new InvalidRequest(`'${label}' must be a number`);
  return Math.min(hi, Math.max(lo, Math.trunc(v)));
}

export function validateStep(raw, step) {
  if (!isObj(raw)) throw new InvalidRequest('a step must be an object', step);
  const action = raw.action;
  if (typeof action !== 'string' || !(action in FIELDS)) {
    throw new InvalidRequest(`unknown action '${String(action).slice(0, 40)}'`, step);
  }
  for (const k of Object.keys(raw)) {
    if (k !== 'action' && k !== 'name' && !FIELDS[action].includes(k)) {
      throw new InvalidRequest(`field '${k}' is not allowed for '${action}'`, step);
    }
  }
  const out = { action };
  const name = str(raw, 'name', step, { allowEmpty: true });
  if (name) out.name = name.slice(0, 200);
  switch (action) {
    case 'goto': {
      out.url = str(raw, 'url', step, { required: true });
      let u;
      try {
        u = new URL(out.url);
      } catch {
        throw new InvalidRequest('invalid url', step);
      }
      if (u.protocol !== 'http:' && u.protocol !== 'https:') throw new InvalidRequest('only http and https urls are allowed', step);
      if (u.username || u.password) throw new InvalidRequest('credentials in the url are not allowed', step);
      break;
    }
    case 'click':
      out.selector = str(raw, 'selector', step, { required: true });
      break;
    case 'fill':
    case 'select':
      out.selector = str(raw, 'selector', step, { required: true });
      out.value = str(raw, 'value', step, { required: true, allowEmpty: true });
      out.secret = bool(raw, 'secret', step);
      break;
    case 'press':
      out.selector = str(raw, 'selector', step);
      out.key = str(raw, 'key', step, { required: true });
      break;
    case 'wait_for': {
      out.selector = str(raw, 'selector', step);
      out.text = str(raw, 'text', step);
      if ((out.selector === undefined) === (out.text === undefined)) {
        throw new InvalidRequest("exactly one of 'selector' or 'text' is required", step);
      }
      const state = raw.state ?? 'visible';
      if (state !== 'visible' && state !== 'hidden') throw new InvalidRequest("'state' must be visible or hidden", step);
      out.state = state;
      break;
    }
    case 'assert_text':
      out.text = str(raw, 'text', step, { required: true });
      out.selector = str(raw, 'selector', step);
      out.negate = bool(raw, 'negate', step);
      break;
    case 'assert_url': {
      for (const k of ['equals', 'contains', 'regex']) out[k] = str(raw, k, step);
      const given = ['equals', 'contains', 'regex'].filter((k) => out[k] !== undefined);
      if (given.length !== 1) throw new InvalidRequest("exactly one of 'equals', 'contains' or 'regex' is required", step);
      if (out.regex !== undefined) {
        if (out.regex.length > LIMITS.maxRegex) throw new InvalidRequest(`'regex' is longer than ${LIMITS.maxRegex} characters`, step);
        try {
          new RegExp(out.regex);
        } catch {
          throw new InvalidRequest('invalid regular expression', step);
        }
      }
      break;
    }
    case 'sleep': {
      const ms = raw.ms;
      if (typeof ms !== 'number' || !Number.isInteger(ms) || ms < 0 || ms > LIMITS.maxSleepMs) {
        throw new InvalidRequest(`'ms' must be an integer between 0 and ${LIMITS.maxSleepMs}`, step);
      }
      out.ms = ms;
      break;
    }
  }
  return out;
}

export function validateNetwork(raw) {
  const n = raw ?? {};
  if (!isObj(n)) throw new InvalidRequest("'network' must be an object");
  const list = (key) => {
    const v = n[key] ?? [];
    if (!Array.isArray(v) || v.length > 256 || v.some((x) => typeof x !== 'string' || x.length > 128)) {
      throw new InvalidRequest(`'network.${key}' must be a list of at most 256 short strings`);
    }
    return v;
  };
  return {
    allow_private: n.allow_private === true,
    deny_cidrs: list('deny_cidrs'),
    allow_hosts: list('allow_hosts').map((h) => h.toLowerCase()),
    block_service_workers: n.block_service_workers !== false,
  };
}

// Renvoie la requête normalisée et bornée ; lève InvalidRequest sinon.
export function validateRun(body) {
  if (!isObj(body)) throw new InvalidRequest('the body must be a JSON object');
  const steps = body.steps;
  if (!Array.isArray(steps) || steps.length === 0) throw new InvalidRequest("'steps' must be a non-empty list");
  if (steps.length > LIMITS.maxSteps) throw new InvalidRequest(`at most ${LIMITS.maxSteps} steps are allowed`);
  const vp = body.viewport ?? {};
  if (!isObj(vp)) throw new InvalidRequest("'viewport' must be an object");
  const screenshot = body.screenshot ?? 'on_failure';
  if (!['on_failure', 'always', 'never'].includes(screenshot)) {
    throw new InvalidRequest("'screenshot' must be on_failure, always or never");
  }
  let userAgent;
  if (body.user_agent !== undefined && body.user_agent !== null) {
    if (typeof body.user_agent !== 'string' || body.user_agent.length > 256 || /[\r\n]/.test(body.user_agent)) {
      throw new InvalidRequest("'user_agent' must be a short single-line string");
    }
    userAgent = body.user_agent || undefined;
  }
  let runId = 'run';
  if (typeof body.run_id === 'string') runId = body.run_id.slice(0, 80);
  const timeoutMs = intIn(body.timeout_ms, 1000, LIMITS.maxTimeoutMs, 60_000, 'timeout_ms');
  const stepTimeoutMs = Math.min(intIn(body.step_timeout_ms, 100, LIMITS.maxStepTimeoutMs, 15_000, 'step_timeout_ms'), timeoutMs);
  return {
    run_id: runId,
    timeout_ms: timeoutMs,
    step_timeout_ms: stepTimeoutMs,
    viewport: {
      width: intIn(vp.width, 200, 3840, 1280, 'viewport.width'),
      height: intIn(vp.height, 200, 2160, 800, 'viewport.height'),
    },
    user_agent: userAgent,
    ignore_https_errors: body.ignore_https_errors === true,
    screenshot,
    network: validateNetwork(body.network),
    steps: steps.map((s, i) => validateStep(s, i + 1)),
  };
}
