# Browser scenarios (Playwright)

A **browser** monitor drives a real Chromium through a short scenario: open a page, log in, click,
wait for text, check the final URL. It catches what a plain HTTP check cannot: a login form that no
longer works, a single page application that renders an empty screen, a JavaScript error behind a
200 response.

The browser does not run inside DumbMonit. It runs in a separate container, the **browser
sidecar**, that only the DumbMonit server can reach. Without the sidecar, browser monitors cannot
run and report a configuration error (no alert).

## Set up the sidecar

The sidecar is the `dumbmonit-browser` service of the Compose file, behind the `browser` profile.

1. Generate a token of at least 32 characters. The sidecar refuses to start with a shorter one.

    ```sh
    echo "BROWSER_TOKEN=$(openssl rand -hex 32)" >> .env
    ```

2. In the `environment` section of the `dumbmonit` service, add:

    ```yaml
    DUMBMONIT_BROWSER_URL: "http://dumbmonit-browser:3000"
    DUMBMONIT_BROWSER_TOKEN: "${BROWSER_TOKEN}"
    DUMBMONIT_BROWSER_CONCURRENCY: "2"
    ```

3. Start both services.

    ```sh
    docker compose --profile browser up -d
    ```

| Variable | Where | Meaning |
|---|---|---|
| `DUMBMONIT_BROWSER_URL` | server | Base URL of the sidecar. Empty (default) means no sidecar. |
| `DUMBMONIT_BROWSER_TOKEN` | server | Bearer token. `DUMBMONIT_BROWSER_TOKEN_FILE` reads it from a file instead. |
| `DUMBMONIT_BROWSER_CONCURRENCY` | server | Scenarios the server runs at once (default `2`). |
| `BROWSER_TOKEN` | sidecar | The same token. |
| `BROWSER_CONCURRENCY` | sidecar | Scenarios the sidecar accepts at once (default `2`). Keep it equal to the server value. |

The sidecar publishes no port. Never expose it: it fetches any URL it is given, within the filter
described below, and the token is its only authentication.

## Write a scenario

A scenario is a list of at most 30 steps, run in order in a fresh browser context (no cookies, no
cache, no storage carried between runs). The first failing step stops the run; the later steps are
reported as skipped. Every step can carry a `name` shown in the results.

| Action | Fields | What it does |
|---|---|---|
| `goto` | `url` | Opens an `http` or `https` URL and waits for the load event. An HTTP status of 400 or more fails the step. |
| `click` | `selector` | Clicks the first element matching the selector. |
| `fill` | `selector`, `value` | Types a value in an input. |
| `press` | `key`, `selector` (optional) | Presses a key (`Enter`, `Tab`, `Control+A`), on an element or on the page. |
| `select` | `selector`, `value` | Chooses an option of a `select` element, by value or label. |
| `wait_for` | `selector` or `text` (exactly one), `state` (`visible` default, or `hidden`) | Waits for an element or a text to appear or disappear. |
| `assert_text` | `text`, `selector` (default: the whole page), `negate` | Checks that the text is present, or absent with `negate`. Retried until the step timeout. |
| `assert_url` | exactly one of `equals`, `contains`, `regex` | Checks the current URL. A regular expression is limited to 512 characters. |
| `sleep` | `ms` (0 to 10000) | Pauses. Prefer `wait_for`. |

Selectors are [Playwright locators](https://playwright.dev/docs/locators): CSS (`#user`,
`button[type=submit]`), `text=Sign in`, `role=button[name="Log in"]`. There is no step to run
JavaScript, upload a file or download one: downloads are refused.

### Limits

| Limit | Value |
|---|---|
| Steps per scenario | 30 |
| Length of any text in a step | 2048 characters |
| Total time of a run | 120 s at most (default 60 s) |
| Time of one step | 30 s at most (default 15 s) |
| Screenshot | one JPEG of the viewport, 512 KiB at most |

Values above the maximum are lowered, not rejected.

## Secrets

Never write a password in a step. Attach a credential of type **web login** (username, password and
an optional TOTP seed) or **API token** to the monitor, then use placeholders in the step values:

| Placeholder | Value |
|---|---|
| `{{secret.username}}` | The login name. |
| `{{secret.password}}` | The password. |
| `{{secret.token}}` | The API token. |
| `{{secret.totp}}` | The current 6-digit code, computed by the server from the seed. The seed never leaves the server. |

```json
[
  {"action": "goto", "url": "https://app.example.com/login"},
  {"action": "fill", "selector": "#user", "value": "{{secret.username}}"},
  {"action": "fill", "selector": "#pass", "value": "{{secret.password}}"},
  {"action": "click", "selector": "button[type=submit]"},
  {"action": "wait_for", "text": "Dashboard"}
]
```

Steps using a placeholder are sent to the sidecar flagged as secret. Their values are replaced by
`***` in every error message and URL of the results, and the matching fields are masked in
screenshots. Changing the steps of a monitor asks you to enter the credential again.

## Network filter (SSRF)

A scenario must not become a way to reach your internal network. Two layers apply:

- The server resolves the host of every `goto` and refuses loopback, link-local, cloud metadata and
  other forbidden addresses before anything is sent.
- The sidecar checks **every** request of the browser (page, scripts, images, frames, redirects,
  pop-ups and WebSockets) against the same list of denied networks, resolving names itself. A
  blocked request is aborted and listed under `blocked_requests` in the run result. Navigations to
  `file:`, `chrome:` or `data:` URLs are refused. The hosts of validated `goto` steps are pinned to
  the addresses that were checked, so a DNS answer that changes mid-run cannot redirect them. WebRTC
  is disabled.

To monitor an application on a private network, enable **Allow private targets**
(`allow_private_targets`) on the monitor. It lifts the filter for that monitor only. Do it only for
scenarios you wrote yourself.

If the main page of a scenario is blocked, the monitor reports a configuration error that names the
setting, not an outage.

## Results and screenshots

Each run records the duration of every step, the failed step, the final URL and the requests that
were blocked. The monitor page shows the last run. After a failure, a screenshot of the viewport
taken at the failing step is kept (the latest failure only; a later success does not erase it).
Password fields and the fields filled from secrets are painted black. A screenshot is a JPEG, never
larger than 512 KiB; if it does not fit, none is kept.

Metrics: `dumbmonit_browser_duration_seconds`, `dumbmonit_browser_step_duration_seconds`,
`dumbmonit_browser_step_success`, `dumbmonit_browser_failed_step` (0 when the run passed) and
`dumbmonit_browser_blocked_requests`, plus `probe_success` and `probe_duration_seconds` like other
probes. A failed assertion, wait or click sets `probe_success` to 0 while the monitor stays
reachable, as for HTTP scenarios.

## Resources

- Image: about 2 GB on disk (Playwright with Chromium).
- Memory: about 300 MB per scenario running at the same time, plus the browser. The Compose service
  is limited to 1 GB (`mem_limit`) and `shm_size` 256 MB, enough for the default concurrency of 2.
  Raise both before raising the concurrency.
- Sidecar at capacity answers 429; the server reports a timeout for that run.
- Run browser scenarios every few minutes, not every few seconds.

## Troubleshooting

| Symptom | Cause |
|---|---|
| "browser sidecar unavailable" | `DUMBMONIT_BROWSER_URL` is empty, the sidecar is not running, or the token differs (401). This is a configuration error, it raises no alert. Check `docker compose --profile browser ps` and the sidecar logs. |
| The sidecar exits at startup | `BROWSER_TOKEN` is missing or shorter than 32 characters. |
| "blocked" / mentions `allow_private_targets` | The page resolves to a private or forbidden address. Enable **Allow private targets** if this is intended. |
| Run reports a timeout while the site is fast | The sidecar was at capacity, or the total budget was too short for the number of steps. |
| Chromium crashes under load | Memory limit too low for the concurrency: raise `mem_limit` or lower the concurrency. |
| `Failed to move to new namespace` in the logs | You enabled `BROWSER_SANDBOX=1` without a seccomp profile that allows user namespaces. Unset it. |
| A selector never matches | Selectors match the first element only. Test it in a browser console with `document.querySelector`, and wait for the page with `wait_for` first. |

Check the sidecar itself: `docker compose exec dumbmonit-browser node -e "fetch('http://127.0.0.1:3000/v1/health').then(r=>r.text()).then(console.log)"`.
