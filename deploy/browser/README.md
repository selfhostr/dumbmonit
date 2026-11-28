# dumbmonit-browser

Playwright sidecar used by DumbMonit `browser` monitors. It runs a scenario (a list of steps) in a
fresh Chromium context and reports the result. It is only reachable by the DumbMonit server.

User documentation: `docs/monitors/browser.md`.

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `BROWSER_TOKEN` | required, 32 characters or more | bearer token the server must send |
| `BROWSER_CONCURRENCY` | `2` | simultaneous runs (1 to 16), the next one gets `429` |
| `BROWSER_PORT` | `3000` | listening port |
| `BROWSER_SANDBOX` | unset | `1` enables the Chromium sandbox (needs a seccomp profile allowing user namespaces) |
| `BROWSER_SHUTDOWN_GRACE_MS` | `30000` | time given to running scenarios after SIGTERM |

## API (v1)

- `GET /v1/health` without authentication.
- `POST /v1/run` with `Authorization: Bearer <token>`, JSON body up to 256 KiB.

Statuses: `200` run done (success or not, see `ok`), `400` invalid request, `401` bad token,
`413` body too large, `429` at capacity, `503` shutting down.

## Develop

```sh
npm ci
npx playwright install chromium   # only for the end to end test
npm test
```

The end to end test is skipped when the Chromium binary is not installed.
