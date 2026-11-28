# Development

DumbMonit is one Rust binary (`crates/server`, which embeds the SvelteKit
build) plus VictoriaMetrics for time series, started by the server from the
binary shipped in the image, and an embedded SQLite database for configuration
and state. One container. Rust code and comments are in French; the web UI is
in English.

## Project layout

```
crates/proto     shared types: Sample, Target, Credential, trait Collector (+ ProbeError)
crates/server    the binary
  api/           axum routes (mod.rs is the route table); spa.rs serves web/build
  auth/          single instance password, HttpOnly session cookie, rate limit
  collectors/    snmp (profiles/*.yaml loaded at startup), proxmox, pbs, synology, agent, uptime
  scheduler.rs   runs every enabled target on its interval through the collector registry
  tsdb/          VictoriaMetrics writer (batched flush) + query proxy; embedded.rs supervises the child VictoriaMetrics
  db/            SQLite + numbered migrations in db/migrations/
  alerting/      rules, state machine (Phase/EffectivePhase), suppression by parent, silences, seasonal baseline
  notify/        22 notification channels, described to the UI by notify/catalog.rs
  crypto.rs      AES-256-GCM for credentials/tokens, key derived from /data/secret.key or DUMBMONIT_SECRET
crates/agent     Linux/macOS/FreeBSD/Windows agent; install/ holds install.sh/.ps1 served by the server
web/             SvelteKit (Svelte 5 runes, Tailwind 4, uPlot, Leaflet), adapter-static SPA fallback, ssr=false
profiles/        SNMP collection profiles, auto-applied by sysObjectID
docs/            this documentation (MkDocs sources, published with Starlight)
```

## Building without a Rust toolchain

Compilation happens in Docker; nothing Rust needs to be installed on the host.

```bash
# Full stack (builds the image: about 10 min cold) — UI on https://localhost:8080
docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d --build

# Rust, inside the builder stage (musl/alpine)
docker build -t dumbmonit-devenv --target builder .
docker run --rm -v "$PWD:/build" -w /build dumbmonit-devenv cargo test
docker run --rm -v "$PWD:/build" -w /build dumbmonit-devenv cargo test -p dumbmonit-server --test it alerts_api::   # one integration test file
docker run --rm -v "$PWD:/build" -w /build dumbmonit-devenv cargo test -p dumbmonit-server test_name          # one test by name
docker run --rm -v "$PWD:/build" -w /build dumbmonit-devenv cargo clippy --all-targets --all-features -- -D warnings
docker run --rm -v "$PWD:/build" -w /build dumbmonit-devenv cargo fmt --all --check
```

With Rust installed locally, `cargo test` and `cargo clippy -- -D warnings`
work directly. Edition 2024, MSRV 1.95; `rustfmt.toml` sets `max_width = 100`
and `use_small_heuristics = "Max"`.

The Docker build is layered: web build → Rust dependencies on dummy sources →
real sources (with a `touch` before the second `cargo build`, otherwise cargo
reuses the dummy artifacts) → agent cross-compiled with cargo-zigbuild for
`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` and
`x86_64-pc-windows-gnu` → `FROM scratch`. `web/build` must exist before
`cargo build`, or the server ships without a UI.

## Web UI

Node 22 is enough on the host.

```bash
cd web && npm run dev        # Vite on :5173, proxies /api to localhost:8080 (the Docker server)
cd web && npm run check      # svelte-check (CI runs this)
cd web && npm run build      # static build into web/build, embedded by rust_embed at cargo build
```

Conventions: Svelte 5 runes only (`$props`, `$state`, `$derived`, `$effect`,
snippets, `onclick`). All network access goes through `src/lib/api/`;
components never `fetch`. Types in `api/types.ts` mirror the Rust structs
exactly and must be kept in sync with `crates/server/src/api/*.rs`. The design
system lives in `src/app.css` (tokens) and `src/lib/ui/` (primitives). Never run two `vite build`s
at once: they wipe `.svelte-kit/output`.

## Translations

The documentation is in English only; the web UI is translated into fourteen languages. The web UI is translated by pull request, like any other contribution. The source language is English (`web/messages/en.json`: flat keys, `{param}` placeholders); each other language is one file, `web/messages/<locale>.json`, compiled by [Paraglide JS](https://paraglidejs.com) into tree-shaken message functions under `web/src/lib/paraglide/` (generated, untracked).

1. Edit `web/messages/<locale>.json` (`fr`, `de`, `es`, `it`, `pt`, `pt-BR`, `ru`, `zh-Hans`), or copy `en.json` to add a new locale.
2. Keep every key and every `{variable}` of `en.json`; translate only the values. Never put HTML markup in a message.
3. Plurals use the message syntax, not hand-built strings: keep the `match`/`plural` structure of the English message and translate each branch.
4. Run `cd web && npm run i18n:check`: it fails when a locale lacks a key or changes its `{variables}`.
5. Open a pull request.

New UI strings are added to `web/messages/en.json` first (that is the only file a feature pull request needs; `npm run i18n:merge` merges transient fragments). A locale appears in the language picker (Settings, Appearance) once its file exists. See `CONTRIBUTING.md` for the key naming convention.

`web/project.inlang/settings.json` points its one plugin (message-format) at
`./node_modules/@inlang/plugin-message-format/dist/index.js` rather than the
jsdelivr URL inlang's own tooling defaults to: a relative, non-`http` path is
read straight from disk, so `npm run build` never reaches out to a CDN —
important since the image build has no guarantee of a warm npm cache offering
that URL, and a homelab tool that needs the internet to rebuild itself is a
bad look.

## Tests and CI

`.github/workflows/ci.yml` runs, on every push and pull request:

1. `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`;
2. `cargo deny check licenses bans` — only OSI-approved licences are allowed
   by `deny.toml`; a copyleft or source-available dependency fails the build;
3. `npm run check` and `npm run build`;
4. the image (`linux/amd64`), build only.

`.github/workflows/release.yml` pushes the same image to
`ghcr.io/selfhostr/dumbmonit`: `:edge` on every push to `main`, `:X.Y.Z`,
`:X.Y`, `:X` and `:latest` on every `v*` tag.

The agent binaries are signed (Ed25519) by `release.yml` with the repository
secret `AGENT_SIGNING_KEY`; the matching public keys, several during a
rotation, are in `crates/agent/signing/release-keys.pem`. Maintainer procedure:
[Signing the agent binaries](development/agent-signing.md).

Integration tests live in `crates/server/tests/`; pure modules (metric
conversion, option parsing, alert state machine, silences) are tested in place
without network.

### Upgrade test

`.github/workflows/upgrade.yml` checks that an existing install survives an
update. It builds the image of the commit, then runs `scripts/upgrade-test.sh`
twice, side by side: from the last published release, which fails the job, and
from the last release of the previous minor version, which only warns (the
project defines no oldest supported version). It runs on every push to `dev` and
`main`, and on pull requests into them that touch the crates, the Dockerfile,
`profiles/`, `integrations/` or the test itself. It is also the release gate:
a tag on `main` is only published after a green run of it.

Each run starts the old image on a fresh `/data` volume and creates real data
through the API: an administrator, an operator and a viewer, two teams,
monitors of several kinds (HTTP, TCP with a parent, ping, SNMP and Proxmox with
encrypted credentials, Home Assistant with an encrypted token, a heartbeat, a
simulated agent that pushes 30 minutes of history), rules (one built-in rule
switched off), silences, an ntfy channel with an encrypted token, a status page
with an incident, instance and notification settings, an API token. A small mock
service (`scripts/upgrade-test/mock.py`) plays Home Assistant and ntfy and
records the tokens it is shown. After the metrics have accumulated, the old
image exports a backup and stops cleanly.

The candidate image then starts on the same volume and the test checks that it is
healthy and logs no error, that every object reads back with the same values and
ids, that each user can still sign in, that the encrypted secrets are readable
(the Home Assistant probe and the channel test must present the right tokens to
the mock, and a backup export, which decrypts every secret, must succeed), that
the heartbeat URL, the agent token and binding, the API token and the public
status page still work, that the VictoriaMetrics history is complete and that
monitoring resumed. Finally the backup made by the old version is restored on a
fresh instance of the candidate, which must come out with the same configuration
and working secrets.

To run it locally you need Docker on Linux (the mock service is reached through
the gateway of a temporary Docker network), `curl`, `git` and `python3`:

```bash
scripts/upgrade-test.sh                                   # last release -> ghcr.io/selfhostr/dumbmonit:edge
scripts/upgrade-test.sh dumbmonit-dev                      # last release -> an image you built
OLD_IMAGE=previous-minor scripts/upgrade-test.sh dumbmonit-dev
OLD_IMAGE=ghcr.io/selfhostr/dumbmonit:0.16.1 scripts/upgrade-test.sh dumbmonit-dev
KEEP=1 UPGRADE_LOG_DIR=./upgrade-logs scripts/upgrade-test.sh dumbmonit-dev   # keep everything to debug
```

It takes about four minutes, most of it waiting for the old version to collect
metrics. On failure it prints the last lines of the logs of the old, new and
restored containers and of the mock service; the complete logs are in
`UPGRADE_LOG_DIR` (in CI, the `upgrade-test-logs` artifact). The `[ok]`, `[KO]`
and `[..]` lines tell what was verified, what broke and what an old version could
not create (not covered, not blocking). The seed data, the checks and the fields
compared live in `scripts/upgrade-test/data.py`: when you add a setting that must
survive updates, create it in `phase_seed` and read it back in `snapshot`.
## End-to-end tests

`web/e2e/` holds Playwright journeys that drive the real image in Chromium, on
a desktop viewport and on a 390 px phone: first-run setup, adding an HTTP
monitor, a webhook channel, an outage and its recovery (alert, webhook, public
status page), the wall, sign-out and sign-in. Any unexpected console error or
uncaught exception fails a step. CI runs them in `.github/workflows/e2e.yml`
and keeps the screenshots, traces and the server log of a failed run.

The suite starts its own container on an empty volume (one per project), plus
two small servers on your machine: the monitored service and a webhook
receiver. Docker is the only requirement besides Node.

```bash
docker build -t dumbmonit-candidate:ci .      # or any image: E2E_IMAGE=...
cd web
npm ci
npm run test:e2e:install                      # Chromium and its system libraries, once
npm run test:e2e                              # both projects, one after the other
npm run test:e2e -- --project=mobile          # one project
npm run test:e2e -- --headed --debug          # watch it
npx playwright show-report ../playwright-report
```

Variables: `E2E_IMAGE` (default `dumbmonit-candidate:ci`), `E2E_HOST` (name the
container uses to reach your machine, default `host.docker.internal`, mapped to
the host gateway), `E2E_SETUP_CODE`. `E2E_BASE_URL` targets an instance that is
already running instead (it must be fresh, started with that setup code and
`DUMBMONIT_ALERT_INTERVAL_SECS=10`). A firewall that blocks the Docker bridge
towards the host stops the monitor and the webhook from working.

The journeys are serial and share one instance: a failed step skips the next
ones. Prefer roles and labels for selectors; add a `data-testid` only when none
fits.

## Adding a collector

1. Implement `Collector` from `crates/proto/src/collector.rs`: `kind()`,
   `probe(&Target) -> Result<Vec<Sample>, ProbeError>` and optionally
   `discover()` for profile detection. Return `ProbeError` variants
   truthfully: `means_down()` decides whether a failure counts as "monitor
   down" (alerted) or as a configuration error (shown on the monitor, not
   notified).
2. Read your options from `Target::tags` in an `options.rs`, with defaults.
3. Register it in `crates/server/src/main.rs` with `registry.register(...)`.
   The scheduler and the API never know concrete types.
4. Describe it in `crates/server/src/api/collectors/`: label, summary,
   examples, credential types, address hint, setup notice and the `options`
   list (key, label, help, placeholder, default, input, choices). The UI builds
   its form from this; a test there checks every registered kind has a notice
   and that the option keys match what the collector reads.
5. Name metrics `<what>_<unit>` without the `dumbmonit_` prefix (the writer
   adds it) and add any new metric a built-in rule targets to the list in
   `alerting/rules/` tests.
6. Add a page under `docs/devices/`.

Samples are `Sample::new(name, value, MetricKind::Gauge | Counter,
timestamp_ms)` with `.with_label(k, v)`; identity labels (`target`, `host`,
`tag_*`) are added by the registry. Do not emit them yourself: name a per-object
label after the object (`proxy_host`, `esxi`, `ceph_host`). A colliding label is
kept as `exported_<name>`, and the options listed for the kind in
`api/collectors/` are not copied as `tag_*`.

## Adding a notification channel

1. Implement `Notifier` (`crates/server/src/notify/mod.rs`): `kind()` and
   `async fn send(&Message) -> Result<(), NotifyError>`. Web channels share
   the HTTP client and helpers in `notify/http.rs`; look at `services.rs`,
   `chat.rs`, `push.rs`, `oncall.rs` and `smtp.rs` for siblings.
2. Add the kind to `CHANNEL_KINDS` and to the `build` match in
   `notify/mod.rs`, which turns a stored `ChannelConfig` (settings + decrypted
   secrets) into a `Notifier`.
3. Describe it in `notify/catalog.rs`: label, summary, `doc_url` anchor, and
   the `settings` and `secrets` fields (key, label, required, input, help,
   placeholder, options, shape, default). The UI renders the form from this
   description, so no UI release is needed.
4. Document it in `docs/notifications.md` under *Supported channels*, with a
   heading whose slug matches the `doc_url` anchor: the UI links straight to
   that anchor in the online documentation.

## Documentation

This site is built with MkDocs and Material for MkDocs from `docs/`
(`mkdocs.yml` at the root). To preview:

```bash
pip install -r docs/requirements.txt
mkdocs serve
mkdocs build --strict   # must pass with no warnings
```

The documentation is published with Starlight at https://dumbmonit.selfhostr.io/docs/. The `page.<lang>.md` files replace the English pages in each language; a page without translation stays in English.
