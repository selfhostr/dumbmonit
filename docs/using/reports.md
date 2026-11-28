# Email reports

!!! note "Optional module"
    Scheduled reports belong to the optional **SLA reports** module, off on a new installation: switch it on in *Settings → System → Modules* ([Settings](settings.md#modules)).

A report is a short email that tells your team how the infrastructure did over
the last day, week or month, without anyone opening DumbMonit. It is weekly by
default; daily and monthly are one click away. Reports go through an **email
(SMTP) notification channel** (see [Notification channels](../notifications.md)),
or, when none is enabled, through the [email server](../notifications.md#email-server)
set in Settings.

Open **Reports** (in the icon rail) to set them up. The page holds the availability (SLA) export of a period, the scheduled reports and the history of their sends; it used to be in Settings, and `/settings/reports` forwards to it. Everything here is admin-only,
reading included, because the page lists the recipients' addresses.

## What a report contains

| Section | Content |
| --- | --- |
| **Headline** | Overall availability for the period, the number of incidents, and the change against the previous period. |
| **Availability by group** | One line per group, then per monitor (the 40 least available at most), computed the same way as on [status pages](status-pages.md): probes by their success rate, hosts and monitors by how often they reported in. Monitors without history are shown as "no data" rather than as 100%. |
| **Incidents** | When each one started, how long it lasted (or that it is still open) and on which monitor. Up to 25. Informational alerts are left out. |
| **Least stable monitors** | The five monitors with the lowest availability or the most incidents. |
| **Comparison** | Availability and incident count against the period of the same length just before. |

The email is plain HTML tables with inline styles and no remote image, so it
renders the same everywhere and nothing is fetched when it is opened. A text
version is sent along. Every monitor name and message is escaped.

## Settings

| Setting | What it does |
| --- | --- |
| **Name** | Shown in the list only. |
| **Enabled** | Off keeps the settings without sending anything. |
| **Frequency** | Daily, weekly (default) or monthly. |
| **Day** | The weekday for weekly reports, the day of the month (1 to 28) for monthly ones. |
| **Hour** and **Timezone** | When the report goes out, in an IANA timezone such as `Europe/Paris`. Daylight saving time is followed. |
| **Recipients** | Up to 20 addresses, free-form: they do not have to be DumbMonit users. Each recipient gets their own email, so nobody sees the others. |
| **Email channel** | Which SMTP channel sends it. By default, the first enabled one, or the email server of the instance when no channel is enabled. Only the mail server of the channel is used: its own recipients are ignored. |

The period is a rolling window that ends when the report is sent: the last 24
hours, 7 days or 30 days. The previous period is the window of the same length
just before.

Click the **i** next to the section title for a reminder of the content.

## SLA reports {#sla}

A schedule of type **SLA report** is the monthly availability report you hand to
a customer or an auditor. It covers the previous **calendar month** (the 1st at
midnight to the 1st at midnight, in the schedule's timezone), so it is always
monthly: the email goes out on the chosen day of the month, with:

- the **PDF**: the name of the organisation (the title of a status page you
  choose, otherwise DumbMonit), the period, availability, incidents and total
  downtime, availability by group and by monitor with the **mean and 95th
  percentile response time** of each probe, and every incident;
- two **CSV files**: the monitors (`availability-2026-10-monitors.csv`) and the
  incidents (`availability-2026-10-incidents.csv`), UTF-8 with a byte order mark
  for Excel. Cells that start with `=`, `+`, `-` or `@` get a leading apostrophe,
  so a monitor name cannot become a spreadsheet formula.

Both attachments can be switched off separately. With an **availability
objective** (for example 99.9), each monitor is marked **Met** or **Missed**
and the report says how many met it. A single group or monitor can have an
objective of its own (**Objectives per group or monitor** in the report's
layout): it replaces the report's objective for those rows, a monitor's own
beating its group's, and an **Objective** column then appears in the tables
and the CSV; without one, the usual words (Healthy,
Degraded, Poor) are used. The report is in English; the figures are the ones
of the status pages.

The PDF is written by DumbMonit itself, with the standard Helvetica fonts and no
library. It is made for paper: print margins, no background fills, table
headers repeated on every page, the name and period at the top of each
following page, and a footer with the generation date and the page number.
Statuses are words (bad ones in bold), so the report reads in black and white.
A logo heads the report when it is a PNG or a JPEG (see the **logo** option
below).

### Customize the report {#customize}

**Customize the report** in a schedule, and the left column of the
[availability report page](#download-a-period), choose:

- the **template**: *Detailed* (every monitor, incident and maintenance window)
  or *Summary* (one or two pages: overall availability against the objective,
  availability by group, or by monitor when there are no groups, the five major
  incidents and total downtime);
- the **sections**: key figures, availability chart, monitors, incidents,
  maintenance windows, response times, notes;
- the **paper**: A4 or US Letter;
- a **title** printed under the name (default: "Availability report") and free
  **notes** (2,000 characters at most);
- the **logo**: *automatic* (the client's in MSP mode, otherwise the logo of
  the status page that brands the report), the *client's*, the *status page's*,
  a *logo uploaded for reports* (PNG or JPEG, 256 KiB at most, one for the
  instance, kept in the backup) or *none*. A client without a logo gets one on
  the Clients page;
- the **monitors covered**: tick monitors, or a whole group with its checkbox
  (a ticked group also covers the monitors added to it later). **All** and
  **None** apply to what the search shows. Every monitor ticked is the whole
  estate; at least one must be. Incidents and maintenance windows follow the
  same scope.

A schedule keeps these options, and so does a [backup](../install/backup.md).

### Download a period

**Open the report**, at the top of Status › Reports, leads to the availability
report page. It builds the same report on demand, for any period of one year
at most (both days included, in the timezone you choose, ending now if it has
not ended). The period has shortcuts: last 7 or 30 days, this month, last
month, last quarter, this year; the default is the previous calendar month. In MSP mode,
choose the client the report is about. Administrators only, and in MSP mode the
members of a client for that client.

The right column shows the report as it will print, rebuilt a moment after each
change. **Print** prints only the report, black on white, with table headers
repeated on each page; **Download PDF** gives the PDF built by the server and **Open PDF** shows it in
a new tab (`disposition=inline`);
**CSV** gives the monitors or the incidents as CSV; **Schedule** opens a new
scheduled report with the same options. The address of the page holds the
options, so a report can be bookmarked or shared with another administrator.

### Sending history

At the foot of the page, **Sending history** lists each scheduled report with
its last send, the next one due and whether the last attempt succeeded (a failed
send shows its error until the next success). Filter it by type and by result.
A failed row has **Retry**, a real send to the saved recipients that clears the
error when it works (once every 30 seconds per report); a sent SLA report has
**PDF**, which builds the report again for the same period, since sent reports
keep no attachment. Each scheduled report has **Duplicate**, which opens a new
form with the same settings.

## Send a preview

**Send a preview** emails the current report to the saved recipients right now,
with `[Preview]` in the subject. It changes nothing about the schedule: the
next report still goes out on time. A preview can be sent once every 30
seconds per report. Save your changes first, since the preview uses the saved
recipients.

## Delivery guarantees

- **At most once.** The slot is claimed in the database before the email is
  built, so a restart never sends the same report twice. When the send fails,
  the error is kept on the report and shown in the list, and the slot is not
  retried; the next one goes out as usual.
- **No catch-up.** If DumbMonit was down at the scheduled time, the report is
  sent when it comes back only if that is within 3 hours of the slot. Later than
  that, it waits for the next slot. A report you create or re-enable never sends
  a slot that is already past.
- An SLA report is built at the time of sending, from the previous calendar
  month: a restart or a late send (within the 3 hours) gives the same report.
- At most 10 reports can exist.
- In demo mode nothing is sent.

Reports are included in [backups](../install/backup.md): the email channel is
matched by name on restore, and a restored report starts fresh (its last-sent
date is not carried over).

## API

See [HTTP API](../reference/api.md#email-reports).
