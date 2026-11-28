# Settings

Settings holds what is administrative: the instance itself, your account, who can sign in and how, the agents, the notification channels, backing the instance up, and the server's health. It is split into six tabs, each with its own address. A rail on a wide screen, a strip of chips on a phone and a search box over the section names help you find a section.

| Tab | Address | What it holds |
|---|---|---|
| My account | `/settings/account` | Appearance (language), Account & security (change password, sign out, two-factor, passkeys, signed-in devices, security log), Push notifications |
| Team & access | `/settings/team` | Users, Teams, Clients, Single sign-on, SAML, SCIM, LDAP |
| Monitoring | `/settings/monitoring` | Agents, Integration packs, [Security score](security-score.md), Locations |
| Notifications | `/settings/notifications` | Channels, Notification policy, quiet hours ([Notification channels](../notifications.md)). Optional on-call schedules (turned on in System) have their tab on the Alerts page: [On-call](../alerting/oncall.md) |
| Wall display | `/settings/wall` | Wall mode: what it shows, and the way in ([Wall mode](wall.md)). Only while the Wall mode module is on |
| System | `/settings/system` | Instance name and time zone, Modules, On-call schedules (on/off), the setup wizard ([Interface and setup wizard](product-modes.md)), Backup, Configuration files, DumbMonit itself, HTTPS and encryption, Network ports, Data & retention, Logs, API & assistants, About |

Each section keeps its own anchor: `/settings#agents` forwards to `/settings/monitoring#agents`. The old links (`/alerts#notifications`) forward to the new place too. `/settings/general` forwards to the System tab. Status pages and reports are not in Settings any more: they live on the **Status** page (`/status`), whose **Reports** tab holds the reports; `/reports`, `/settings/status` and `/settings/reports` forward there. The wall's news and widgets are gone: their old links (`#news`, `#widgets`) open the Wall display tab.

## Appearance {#appearance}

The language of the interface, saved in this browser. The console has a single
dark look, so there is no theme to pick; public status pages keep their own
light and dark themes.

Two switches, **Show GitHub link** and **Show documentation link**, control the buttons
at the bottom of the sidebar. Both are shown by default; the choice is kept in this
browser, like the time format and the density.

## Modules {#modules}

*System → Modules* (administrators). Some parts of the console are optional and
**off on a new installation**: switch on only what you use.

| Module | What it adds |
|---|---|
| Wall mode | The full-screen room monitor, the Wall display tab and wall display accounts ([Wall mode](wall.md)) |
| SLA reports | Scheduled reports and the monthly availability report, in the Status page |
| Status pages | Public status pages, incidents and maintenance announcements |
| SLOs | Availability targets and their error budget, in the Status page |
| Threat map | The world map of stopped attacks in Monitors → Security. It also needs a source of attack data (CrowdSec, a firewall's live map or Cloudflare): without one nothing is drawn, even with the module on |

A module that is off keeps its data. It disappears from the navigation, the
settings, the command palette and the overview; its page tells administrators
that the module is off (anyone else sees "not found"), its API answers `404`,
and a public status page answers `404` while Status pages is off. Wall display
accounts are refused while Wall mode is off.

When you upgrade an existing instance, each module that is already in use is
switched on: Status pages when a page exists, SLOs when one is defined, SLA
reports when a report schedule exists, Wall mode when a wall display account or a
wall music link exists. The threat map stays off until you enable it. The choice
is part of the [backup](#backup). `GET /api/settings/features` reads it,
`PUT /api/settings/features` (administrators) changes it.

## Wall display {#wall}

**Open wall mode**: the console's board, full screen, for a monitor in the room.
++esc++ leaves it, and the command palette opens it from anywhere. The wall has
no news or widgets any more; an optional night city can sit behind its verdict.
See [Wall mode](wall.md).

## Account & security

This is your own account. The first one is created on first start, and more
can be added below. To change your password, click **Change password** and fill in the current password, the new
password (at least 12 characters; a whole phrase is safer than a complicated
word) and its confirmation. You should see *Password changed*, and every other session is signed out. Accounts that sign in through the identity provider have
no password here. **Sign out** ends this session.

Login is rate-limited per client address and per account: after five failed
attempts, each further attempt waits longer (30 s, doubling, up to 5 minutes).
Behind a reverse proxy, set `DUMBMONIT_TRUSTED_PROXIES` so the limiter sees
the real client address rather than the proxy's (see
[Configuration](../reference/configuration.md)). Sessions last 30 days
(`DUMBMONIT_SESSION_DAYS` changes it, from 1 to 365). A
forgotten password is reset with `DUMBMONIT_RESET_PASSWORD=1`: see the
[FAQ](../faq.md#i-lost-the-password).

### Two-factor authentication

Accounts that sign in with a password can add a second factor: a six-digit
time-based code (TOTP) from an authenticator app — Aegis, FreeOTP, Google
Authenticator, 1Password, Bitwarden… **Set up** asks for your password, shows
a QR code to scan (or the key to type), and enables the second factor once you
enter a first code from the app. Eight **recovery codes** are then shown once:
save them, each one signs you in a single time if the phone is lost. The
sign-in screen asks for the code (or a recovery code) after the password;
five wrong codes cancel the attempt and you start over from the password, and
the usual sign-in rate limit keeps counting across attempts. A code is
accepted once: the one you just used (including the one that enabled the
second factor) does not sign you in again — wait for the next one.

**Download .txt** next to **Copy codes** saves them as a text file, built in
the browser. **Replace recovery codes** gives a fresh set without turning the
second factor off: confirm with a code from the app (or your password), and
the old codes stop working at once. **Disable** asks for the password again. An admin can reset
another user's second factor from **Users** (*Reset 2FA*) when both the phone
and the codes are gone — that signs the user out everywhere and leaves the
password alone. The secret is stored encrypted with the instance secret;
codes are hashed. Single sign-on accounts get their second factor from the
identity provider, not here.

### Signed-in devices

Every browser where your account is signed in: browser and system, how it
signed in (password, passkey, SSO, LDAP), when, the last activity (updated at
most every five minutes) and the address it came from. The one you are using
is marked *This device* and closes with **Sign out**; **Sign out** on another
row closes that one, and **Sign out my other sessions** closes them all at
once. Changing your password does the same.

Admins find the **audit log** in its own section of the *Team* tab; see
[Audit log](#audit-log).

## Audit log {#audit-log}

Admin-only, in the *Team* tab. It records who did what and from where: sign-ins,
failures and sign-outs, password and two-factor changes, token and account
changes, backups exported, restored or downloaded, single sign-on and LDAP
settings, notification channels created, changed or deleted, and more. Each
entry has a readable label; an event this version does not know is shown as its
raw name. Secrets never appear in the details.

Filter by **account**, **event**, **address** and **period** (UTC), and press
*Load more* to read further back. **Export CSV** and **Export JSON** download
what the filters select (the export itself is logged). In the CSV, a cell that
a spreadsheet could read as a formula starts with an apostrophe.

It keeps the last 5 000 entries by default; `DUMBMONIT_AUDIT_KEEP` changes that
(100 to 1 000 000, see [Configuration](../reference/configuration.md)).

## Push notifications

Native notifications on this phone or computer, even with DumbMonit closed:
**Enable on this device**, then **Send a test**. Every account manages its own
devices here (viewers too); which alerts reach them is set by a **Web Push**
channel in Settings → Notifications. The **i** button sums up the requirements:
HTTPS (or localhost), Home Screen installation on iPhone and iPad, and the
push service domains the server must reach. Full guide:
[Push notifications (PWA)](push-notifications.md).

## My notifications {#my-notifications}

Alerts sent to you personally, on top of the team channels — every account
sets its own, viewers and operators included (wall displays have no
settings). Turn on **By email** (to your account's address, through the
[email server](../notifications.md#email-server) of the instance; an admin
adds the address in **Users**) and/or **On my push devices** (the devices
subscribed under *Push notifications* just above), then choose:

- **Send** — the minimum severity: everything, *Advisory and up*, or
  *Warning only*.
- **Same alert again no sooner than** — a minimum delay between two
  messages for the same alert.
- **Tell me when it clears** — off, you only hear about problems.
- **About** — everything you can see, your teams' folders, or the folders
  (sub-folders included) and tags you pick. Whatever the choice, an alert on
  a monitor your teams do not show never reaches you.
- **Quiet hours** — days and a time span in this browser's time zone, which
  follows daylight saving. During quiet hours only *Warning* alerts reach
  you; the rest is not sent to you and stays in the alert list.

**Send me a test** tries each means saved as on. The rules of the channels
apply here too: one message per monitor, flapping alerts held back, the
hourly cap of the notification policy, and a recovery only for what was
announced to you. Details in
[Personal notifications](../notifications.md#personal-notifications). The
choices are part of the backup.

## Users

Admins only. The accounts that can sign in, and their role:

| Role | Reads everything | Handles alerts (ack, snooze, ignore, maintenance windows, clear history) | Changes configuration (monitors, rules, channels, status pages, settings, backups, tokens, accounts) |
|---|---|---|---|
| **Viewer** | yes | no | no |
| **Operator** | yes | yes | no |
| **Admin** | yes | yes | yes |
| **Wall display** | only what the wall shows | no | no |

A viewer sees every page with every control that would change something
hidden. A few things stay admin only even to read: API and agent tokens,
status page subscriber emails, and monitor options that look like secrets
(an option whose name contains `key`, `token`, `password`…, the extra headers
of an HTTP check, the password of a `https://user:pass@host` address), which
non-admins read as `***`. An operator is the on-call role: the alert controls are there, the
configuration screens read *Operator — config is admin only*. Only an admin
creates accounts or changes a role — an operator cannot promote anyone,
themselves included. The *Users*, *Single sign-on* and security log sections
stay admin only. **Add user** asks for a username, an
optional display name, a role and a password; the button beside the password
field makes up a random one. The password is shown once, to hand over.

Each row carries the role, whether the account signs in with a password or
through the identity provider, whether two-factor is on, the email address
when there is one, and the last sign-in. Above the list, search by name,
username or email, show only the accounts **Without 2FA**, and sort by name,
role or last sign-in. **Edit** changes the display name, the optional email
address (to know whom to reach; it is never used to sign in), the role and
the password (a reset signs that user out everywhere and shows the new
password once). **Sign out everywhere** closes every session of the account
(for your own, the one you are using stays open). The **Enabled**
switch suspends an account without deleting it; **Reset 2FA** appears when the
account has a second factor; **Delete** removes it for good. The server keeps
at least one active admin, and the controls reflect that rule rather than
reporting it as an error: the last admin cannot be demoted, disabled or
deleted, and you cannot disable or delete your own account.

**Reset link** (on the row of an account that signs in with a local
password) creates a single-use link to `/reset`, where the person chooses a
new password with the same rules as everywhere. It is emailed to the
account's address when it has one and an email server is configured, and
shown once in every case, to copy and hand over. A new link replaces the
previous one. Using it signs the account out everywhere; two-factor
authentication stays on. Single sign-on and LDAP accounts change their
password with their provider, and wall displays get a new password through
**Edit**: neither has a reset link.

### Custom roles {#custom-roles}

When a built-in role gives too much or too little, an admin defines a
**custom role** under **Settings → Team & access → Custom roles**: a name,
a description and a set of permissions picked from a fixed list. In
**Users**, the role picker then offers the custom roles below the built-in
ones; choosing one replaces the built-in role of that account.

| Permission | What it opens |
|---|---|
| **View** | Everything a viewer reads. Always part of a custom role. |
| **Handle alerts** | What an operator does: acknowledge, ignore and silence alerts, take on-call shifts, add change markers, run remediation actions already defined. |
| **Manage monitors** | Create, edit, import and delete monitors, monitor templates, alert rules and SLOs, run a probe. Device credentials can be entered, never read back: options that look like secrets still read as `***`, and sending `***` back keeps the stored value. |
| **Manage notifications** | Notification channels, the notification policy and message templates. Not the email server. |
| **Manage status pages** | Status pages and incidents. Not the page password, nor the subscribers. |
| **Manage maintenance** | Maintenance windows. |
| **View reports** | Read the scheduled reports and download their PDF. |

Accounts, roles, teams, API and agent tokens, single sign-on, LDAP and SCIM,
backups, instance settings, the email server and actions on machines (agents,
containers, remediation set-up) stay admin only: no permission opens them.
The server checks every request against the role, route by route, and
refuses anything the role does not list; the interface only hides what the
account cannot do. Each permission's routes are listed by
`GET /api/roles/permissions` and marked `x-permissions` in the
[API specification](../reference/api.md).

- Changing a role applies at once to every account that has it.
- A role still given to an account cannot be deleted: give those accounts
  another role first.
- With teams, the permissions also set the ceiling of the account: *Manage
  monitors* allows up to *manage* on its folders, *Handle alerts* or *Manage
  maintenance* up to *operate*, anything else *read*. A team-restricted
  account still only reaches the pages teams can use.
- *Require two-factor for admins and operators* also covers accounts whose
  custom role changes something.
- A custom role is given in **Users**; single sign-on, LDAP and SCIM keep
  handing out built-in roles. When the groups only say *viewer*, the custom
  role of the account stays; when they make it an operator or an admin, the
  custom role is removed.
- Custom roles and who has them are part of the backup.
- A wall display cannot have a custom role.

### Invitations {#invitations}

**Invite** asks for an email address, a role (viewer, operator or admin —
a wall display is created with **Add user**) and the teams to join, and
sends a link to `/invite`. There the person picks a username (the part of
the address before `@` is suggested), a display name and a password, and is
signed in at once. When single sign-on (OIDC or SAML) is enabled, the page
also offers the provider's button: the account is then created without a
password, with the invited address as username, and is linked at the first
sign-in when the provider presents that address as verified. Without an
email server, the link is shown once to copy and send yourself.

Open invitations are listed under the form, expired ones included: **Send
again** creates a new link with a new expiry (the previous link stops
working), **Revoke** withdraws it. Inviting the same address again replaces
the open invitation; an address already used by an account is refused.
Only admins invite: team rights do not cover accounts.

### Forgot password by email {#forgot-password}

On the sign-in screen, **Forgot password?** asks for a username or an email
address and emails a reset link when:

- the switch **"Forgot password?" by email** in **Invitations** is on (it is
  by default),
- an [email server](../notifications.md#email-server) is configured,
- and DumbMonit knows a safe address for the link: the public URL of the
  [notification policy](../notifications.md) (or `DUMBMONIT_PUBLIC_URL`),
  otherwise the address an admin last used to open these settings. The
  address a stranger's browser sends is never used, so nobody can make
  DumbMonit email a link that leads elsewhere.

Otherwise the link points to the [FAQ](../faq.md#i-lost-the-password), as
before. Only an active account with a local password and an email address
gets a link — never a single sign-on, LDAP or wall-display account. The
screen answers the same way, after the same delay, whether an account
matches or not, and each request counts against a limit per address and per
account. Links work once; a reset link lasts 30 minutes and an invitation 7
days by default (**Reset links last** and **Invitations last**). The links
hold a 256-bit secret, of which the server only keeps a hash. Every step —
request, link created, reset, invitation created, resent, revoked, accepted,
and attempts with an unusable link — is recorded in the
[audit log](#audit-log). The settings and open invitations are part of the
backup; reset links are not (they last half an hour).

### Wall display accounts {#wall-display-accounts}

The **Wall display** role is for a screen that stays on in a room (a TV, a
lobby or control-room monitor). Signed in with such an account, the browser
goes straight to [wall mode](wall.md) and stays there: every other address
leads back to the wall, the wall's **Exit** becomes **Sign out** (two taps,
so a stray one does nothing) and ++esc++ does nothing. Device tiles and
problems on the wall are not links.

The server enforces the same: the account reaches only what the wall reads
(devices without their address, options or note, alerts, rules, metrics,
incidents and maintenance windows, the instance name, and the weather) and every other call answers `403`, reads included. Team
rights limit which monitors it shows, as for any account. It cannot create
API tokens, and a token whose owner becomes a wall display stops working.

- **Session.** A wall display stays signed in as long as it shows the wall:
  its session is renewed while it is in use, so `DUMBMONIT_SESSION_DAYS` is
  how long it may stay switched off or idle before it has to sign in again.
  Disable the account, reset its password, use **Sign out everywhere**, or
  give it another role to sign the screen out: leaving the wall display role
  closes its sessions at once.
- **Two-factor.** The two-factor requirement below never applies to wall
  displays: nobody stands in front of the screen to type a code.
- **Single sign-on, LDAP and SCIM** can hand out the role too: see the
  *Wall display groups* below and the [SCIM](scim.md) default role.

### Require two-factor for admins and operators

The switch at the top of **Users** makes two-factor authentication mandatory
for admins and operators who sign in with a local password. One of them
without a second factor can, after signing in, only set it up (or sign out):
the rest of the interface waits, and the API answers `403` with
`"code": "totp_setup_required"` until it is done. Viewers and wall displays
are not concerned.
Sign-ins through single sign-on (OIDC, SAML) and LDAP are exempt — the
identity provider handles their multi-factor authentication — and so are
passkey sign-ins, which already prove possession and the user. API tokens
are not affected. The setting is part of the backup.

## Single sign-on

Admins only. Sign-in through an OpenID Connect provider (Authelia, Authentik,
Keycloak, Pocket ID, Google Workspace…). Fill in the *provider name* (it
labels the "Continue with …" button on the sign-in screen), the *issuer URL*
— discovery is read from `{issuer}/.well-known/openid-configuration` — the
*client id*, the *client secret* (stored encrypted, never shown again) and the
*public URL* browsers reach DumbMonit at. The form builds the redirect URI
from that public URL: copy it and register it with your provider.

**Test discovery** reads the provider's configuration without signing anyone
in, lists the endpoints it found, and warns when the provider signs with
neither RS256 nor ES256 — the only two signatures DumbMonit accepts. Settings
saved here take precedence over the `DUMBMONIT_OIDC_*` environment variables,
which only apply while nothing is saved; **Forget saved settings** clears them
and falls back to the variables when they exist.

**Advanced.** The rest of the form sits under *Advanced*: *Scopes* names what
to ask the provider for, *Groups claim* the token claim that lists the user's
groups, and *Create accounts on first sign-in* decides whether an unknown
person gets an account. It is **off by default**: only accounts that already
exist can sign in until you turn it on.

**Roles.** *Admin groups* lists the groups whose members become admins;
*Operator groups* the groups whose other members become operators; everyone
else is a viewer. Leave both empty to keep managing roles under **Users**.
With only *Operator groups* filled in, the groups never touch the admin role:
admins stay admins, the others become operators or viewers. *Wall display
groups* lists the groups whose members, when they are neither admins nor
operators, become [wall displays](#wall-display-accounts); filled in alone,
it only decides that role (its members become wall displays, a wall display
that leaves it becomes a viewer, other accounts keep their role). Roles are
re-evaluated at each sign-in, except that the last active admin is never
demoted. The matching environment variables are `DUMBMONIT_OIDC_ADMIN_GROUPS`,
`DUMBMONIT_OIDC_OPERATOR_GROUPS` and `DUMBMONIT_OIDC_WALL_GROUPS`.

**Which account a sign-in lands on.** The identity is remembered by its
provider subject after the first sign-in. On a first sign-in:

- An existing account is linked only when the provider asserts
  `email_verified: true` and that email is the account's username. A
  `preferred_username` or an unverified email never links: on most providers
  the user chooses those, so `admin` from the provider must not become *your*
  `admin`.
- A local **admin that has a password is never linked automatically**, even
  on a verified email: the sign-in creates a distinct account instead. Keep
  managing that admin with its password, or create a separate SSO admin
  through the admin groups.
- Otherwise, with *Create accounts on first sign-in* on, a new account is
  created (username from the provider, suffixed `-1`, `-2`… when taken). With
  it off, only the verified-email link above can sign in.

**Account creation is off unless you choose it.** New installs start with
*Create accounts on first sign-in* off, and so does any configuration where
it was never set — settings saved before the option existed, or
environment-only setups without `DUMBMONIT_OIDC_AUTO_CREATE`. Earlier releases
treated "never set" as on; the Single sign-on panel now says when this
default applies. A value you saved explicitly is kept as it is.

## LDAP / Active Directory

Admins only. Sign-in with directory usernames and passwords on the usual
sign-in screen, roles from the directory groups, accounts created at the first
sign-in. See [LDAP / Active Directory](ldap.md).

## SAML, SCIM and teams

Admins only. SAML 2.0 single sign-on, SCIM provisioning, and teams that limit
each account to some folders of monitors. See [SAML 2.0](saml.md),
[SCIM provisioning](scim.md) and [Teams and permissions](teams.md).

## Agents {#agents}

Only what is shared by every [agent](../devices/agent.md), for admins: the
encrypted agent channel (certificates and their authority, see
[Agent security](../install/agent-security.md)), automatic agent updates, and
the enrolment tokens already issued. Each token shows its scope, creation date
and last use; **Revoke** stops every agent using it at its next push, without
deleting the monitors.

Adding a machine (token, install command, first connection, a route through a
relay, the Active Directory kit) happens in the add flow, **New monitor →
Agent** (`/targets/new?kind=agent`), not here. The agents themselves are on
their own pages.

Under **Enrolment approval**, choose what happens to a machine that enrols with a token (accept it, ask an administrator, or accept it when it belongs to the token's Active Directory domain), and the hourly limit per token. Waiting machines are listed there. A fleet token that enrolled nothing for 30 days goes to sleep and shows **Asleep** with a **Reactivate** button. Details: [Protecting enrolment](../install/agent-security.md#protecting-enrolment).

## HTTPS and encryption {#https}

Admin-only, in **Settings → System**. Shows the certificate the web port serves (self-signed or yours), its fingerprint and expiry, and a download of the certificate (PEM). It also lists every connection that still travels in clear or without certificate checks, with a link to where to change it. See [HTTPS and encryption](../install/https.md).

## Network ports {#ports}

Admin-only, in **Settings → System**. The ports DumbMonit listens on, computed from the running configuration: the web interface (HTTPS), the agent channel, syslog, SNMP traps, the bundled storage, and the gateway of each relay agent. For each one: who must reach it, whether it may face the Internet, and whether it is on, off or loopback only. Below the list, a ready-to-paste `ports:` block for Docker Compose and `ufw` rules. See [Network ports](../install/ports.md).

## Security score {#security-score}

In **Settings → Monitoring**, below the agents: a score for how well the instance and its monitored hosts are hardened. See [Security score](security-score.md).

## Reports

On the **Reports** tab of the **Status** page (`/status`; it used to sit in Settings → Notifications): the availability (SLA) export of a period, the periodic availability emails for the team and the history of their sends. Admin-only. See [Email reports](reports.md).

## Data & retention {#data-retention}

Admin-only. Shows how much disk the metrics (`vm`), the 5-minute aggregates
(`vm-long`), the log store (`vlogs`), the database and the backups take, the
free space, and the growth per day with the number of days left before the
volume is full.

* **Raw metrics retention** (7 to 3650 days). Read-only when an external
  VictoriaMetrics is used: set it there. Lowering it deletes the older raw data
  at the next VictoriaMetrics retention cycle.
* **Long-term history**: on or off, its retention (730 days by default) and its
  state (last window written, history copy progress). Graphs, status pages and
  reports reaching past the raw retention are answered from it.
* **Log store**: off by default. Turning it on starts the embedded VictoriaLogs
  and opens the admin-only **Logs** page, a LogsQL search over the server's
  own logs.

Saving a retention restarts the embedded process concerned for a few seconds;
samples are buffered meanwhile. See [Sizing](../reference/sizing.md) and
[External VictoriaMetrics](../reference/external-victoriametrics.md).

## Backup

Three things, in the order you meet them.

A **warning you cannot miss**: `/data/secret.key` is what decrypts monitor
credentials, and a copy of the database without it restores an instance that
cannot talk to anything. The wording changes when the secret comes from
`DUMBMONIT_SECRET` instead — there is then no file to copy, and it is your
password manager that has to hold it.

**Export the configuration** lists what the bundle would contain, with a count
per section, then asks for a passphrase twice. The file that comes down holds
every credential of this instance, encrypted with that passphrase alone: keep
it where you keep passwords. Account passwords and 2FA secrets are left out
unless you turn the switch on.

**Restore a bundle** takes the file and its passphrase and shows a **dry run**
first: a table of what would be created, updated and left alone, section by
section, with a note for anything it cannot place. Nothing is written until you
press *Restore for real*.

**Scheduled local backups** shows whether they are on, where they go, the last
run and whether it succeeded, and the files kept with their size. **Back up
now** writes one immediately — the thing to do before an upgrade. The schedule
(on or off, hours between backups, copies kept) is editable here; a setting
fixed by a `DUMBMONIT_BACKUP_*` variable is shown as managed by the environment
and cannot be changed from the screen. Each copy on disk can be downloaded.

Everything here is admin-only. See [Backup and restore](../install/backup.md).

## DumbMonit itself {#self-monitoring}

The state of the engine right now — scheduler, SQLite database, metrics
database — with the free space on the data volume, the database size and, for
the embedded VictoriaMetrics, how many times it was restarted. Built-in rules
already warn you when any of these goes wrong.

Below, the **outgoing heartbeat**: a URL of yours (Healthchecks.io, an Uptime
Kuma push monitor, Better Stack…) that DumbMonit calls at a regular interval,
only while it is healthy, so that someone outside notices when this host goes
quiet. **Test** sends one call right away; **Signal failures** calls the
failure URL instead of staying silent. The URL is stored encrypted and never
shown again, only its host. Changing it is admin-only. See
[Watching DumbMonit itself](../install/self-monitoring.md).

## API & assistants

One kind of token for both: MCP clients and the
[REST API](../reference/api.md). Name a token, pick its scope and create it —
it is shown once. A **read** token can only look; a **read and write** token
can also change things — silence a monitor, run a probe, add or edit monitors,
rules and channels — but never accounts, sign-in settings or other tokens.

Under **Connect**, three tabs hold the snippets, filled with the token while
it is still on screen: *Claude* (the `claude mcp add` command and the
`claude_desktop_config.json` block), *ChatGPT* (the MCP server URL and the
authorization header, with a warning when this page is not served over HTTPS —
ChatGPT connects from OpenAI's servers, so the address must be reachable from
the internet) and *Cursor / other*, the same shape for any client that speaks
Streamable HTTP with a bearer header. **API access** shows the same token on
the REST API as a `curl` example, and points at the routes a Prometheus or a
Grafana scrapes with it. See [Connect an assistant](assistant.md) and
[Scraping DumbMonit](../reference/metrics.md#scraping-dumbmonit).

## About

Version of the server and the health of its two stores, the database (monitor
setup and alert history) and VictoriaMetrics (every measurement in the
charts), as reported by `GET /api/health` and refreshed every 15 seconds. A
store that stops answering shows a warning with the error and what to check.
The section also links to this documentation and names the licence, MIT.

For administrators, **Effective configuration** lists what the server runs with,
without any secret: data directory, ports, VictoriaMetrics (embedded or
external, host only), backup schedule, log filter. The same list is printed by
`dumbmonit --check-config` (see [Configuration](../reference/configuration.md#checking-your-configuration)).

**First run**, folded at the bottom, times the road to the first alert on this
instance: started without an account, administrator created, first monitor,
first channel, first message delivered (a test counts) and first real alert
delivered, each with its delay from the start. The times are recorded once,
stay on the instance and are never sent anywhere (`GET /api/onboarding`,
`first_run`). See [First alert in five minutes](../install/quickstart.md).
