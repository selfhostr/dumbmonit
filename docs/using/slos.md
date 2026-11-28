# SLOs and error budgets

!!! note "Optional module"
    SLOs are an optional module, off on a new installation: switch them on in *Settings → System → Modules* ([Settings](settings.md#modules)).

A service level objective (SLO) says how available something has to be over a
rolling window: "the public API is up 99.9 % of the time over 30 days". What the
target leaves over (0.1 % of 30 days, so 43.2 minutes) is the **error budget**.
DumbMonit tracks how much of it is left and warns you when it is being spent
too fast, before the target is missed.

SLOs live under **Status → SLOs**. Everyone can read them; administrators
create, edit and delete them. Accounts restricted by [teams](teams.md)
only see the SLOs whose monitors all belong to their teams, and an operator can
only create or change an SLO made of monitors their teams let them manage.

## Create an SLO

**New SLO**, then:

| Field | What it does |
| --- | --- |
| **Name** | Unique. Shown in the list and in burn-rate alerts. |
| **Target availability** | A percentage strictly between 0 and 100, such as `99.9`. The line under the field says what it allows: *Allows 43.2 min of downtime over 30 days*. |
| **Rolling window** | 7, 28, 30 or 90 days, always ending now. |
| **Monitors** | One monitor, several, or whole folders, with the same picker as the [SLA report](reports.md). A ticked folder also covers the monitors added to it later. |
| **Burn-rate alerts** | On by default. Off, the SLO is still measured and shown but raises no alert. |

## How it is measured

Availability is measured exactly as on [status pages](status-pages.md) and in
SLA reports: an uptime probe (HTTP, ping, TCP, DNS…) by the share of successful
checks (`dumbmonit_probe_success`), any other monitor by how often it reported
in (`dumbmonit_up`, in five-minute slots). Time before a monitor's first
measurement does not count as downtime.

An SLO covering several monitors takes the **average** of their availabilities,
each monitor with the same weight. Disabled monitors are left out.

For each SLO the list shows:

- **target and window**, for example *99.9 % over 30 days*;
- **attained**: the availability over the window;
- **error budget**: a bar and its words (*62 % of the budget left*, *Budget
  spent*). The bar is green with room to spare, amber below 25 %, red once
  spent; the words always say it, so colour is never the only signal;
- **burn rate**: how fast the budget is going right now.

Open a row for the detail: the **budget burn-down** over the window (100 % at
the start of the window, falling with each outage, below 0 once the target is
missed), the minutes of allowed downtime already used, and the burn rate over
5 min, 30 min, 1 h and 6 h.

When VictoriaMetrics does not answer, the list still loads and says *Metrics
unavailable*.

## Burn rate

The burn rate is the downtime over a window divided by the downtime the target
allows. At **1×**, the budget runs out exactly at the end of the window; at
**14.4×**, one hour spends 2 % of a 30-day budget.

## Burn-rate alerts

The built-in rule **Error budget burning** (`slo_burn_rate`, kind `slo_burn`)
follows the multi-window method of the Google SRE workbook. A long window says
the spending matters; a short one says it is still going on. Both have to be
over the line:

| Burn | Condition | Severity |
| --- | --- | --- |
| Fast | at least 14.4× over the last hour **and** over the last 5 minutes | Warning (`critical`) |
| Slow | at least 6× over the last 6 hours **and** over the last 30 minutes | Advisory (`warning`) |

The multipliers are the same whatever the window of the SLO. On a 7-day SLO a
fast burn spends about 8.6 % of the budget per hour; on a 90-day SLO, 0.7 %.

The alert is held for one minute, then goes through the usual pipeline:
grouping, [notification policy](../alerting/notifications.md), quiet hours,
[maintenance windows](../alerting/maintenance.md) and acknowledgement. A slow
burn that turns fast notifies again, as a warning, under the same alert. The
alert clears as soon as the short window is clean again, without waiting for
the hour or the six hours to empty. It is attached to the first monitor the SLO
covers (the lowest id, so the alert keeps its identity), shows the SLO's name
in its message, and links to the SLO page.

You can retune the rule like any other in **Alerts → Rules**: its threshold
(`1` opens on a slow burn, `2` on a fast burn only), its severity, hold and
channels. It ignores its query.

## API and backup

`/api/slos` lists, creates, updates and deletes SLOs; `GET /api/slos/{id}` adds
the monitors covered and the burn-down points. See the
[HTTP API](../reference/api.md). SLOs are part of the
[configuration backup](../install/backup.md): their monitors are matched by kind
and address on the instance you restore to.
