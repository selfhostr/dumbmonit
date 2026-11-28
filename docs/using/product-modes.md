# Interface and setup wizard

DumbMonit has one interface: a dense, dark console made to stay open all day.
Earlier versions offered a choice between a "Homelab" and an "Enterprise"
interface; there is only one now, and nothing was lost in the merge.

## The console

- An **icon rail** on the left lists every destination, status pages and reports
  included. At its foot sit the documentation and GitHub links (both can be hidden
  in *Settings → My account → Appearance*), your account (the round button with
  your initials) and Settings. A phone gets a tab bar with the
  essentials and **More**.
- The **header** shows where you are (instance name, page, monitor), a **LIVE**
  switch that pauses the console's own refreshes, the time range on the
  overview, the command palette, **New monitor**, **wall mode** (the screen
  icon) and the context panel toggle.
- The **context panel** on the right, like an inspector, shows what needs
  attention, the monitor selected in a table (with **Check now** and **Silence
  1 h**), the nearest forecast and who gets told. Press `i` to show or hide it;
  on narrower screens it opens as a drawer.
- The **overview** opens on a thin night-sky banner whose one line says how
  things stand right now ("pve-02 is down. 3 guests held back, alert sent." or
  "All 146 monitors up."). Below it: five figures (monitors up, availability
  over 30 days, p95 response time, open alerts, mean time to resolve), the
  **health river** (one strip per monitor, one cell per slot of the range, worst
  monitors on top) and a **monitor table** sorted worst first. The table filters
  by state, type and text, lets you add columns (address, folder) and remove
  others, and is driven by the keyboard: `j` / `k` move, `Enter` selects, `o`
  opens.
- **Wall mode** is a black board for a screen in the room: the health river, the
  five figures, open alerts and who is on call.
- Shortcuts: `g o` overview, `g d` monitors, `g a` alerts, `g m` topology map,
  `g x` security, `g l` logs. Press `?` for the full list.

States are always written next to a shape (circle up, diamond degraded, square
down), never shown by colour alone. Public status pages keep their own light and
dark themes.

## Density

*Settings → My account → Appearance* offers two densities, **Comfortable**
(default) and **Compact**. Compact tightens the spacing and row heights of the
whole console so more monitors, alerts and log lines fit on one screen. Like the
time format and the week start, it is kept in the browser: each person picks
their own.

## Optional modules

Wall mode, SLA reports, status pages, SLOs and the threat map are optional
modules, **off on a new installation** and switched on in *Settings → System →
Modules* (or in the setup wizard). A module that is off disappears from the
navigation and its API answers `404`; its data is kept. An instance that is
upgraded keeps on the modules it already uses. Details in
[Settings](settings.md#modules).

## Setup wizard

The wizard opens after the first administrator signs in on a new instance. It is a
full-page screen without the console around it: one step at a time, a step
counter, **Back** and **Continue**, and the step is kept in the address
(`/welcome?step=monitors`) so the browser's Back button works. Run it again any
time from **Settings → General → Run the setup wizard**.

1. **Your first alert**: something to watch, where the alert should reach you
   (the same channel form as in Settings, so every channel type is there), and a
   test that proves it arrives.
2. **Instance**: name shown in the header, default language and time zone.
3. **First monitors**: scan your network in place, import from another tool, or
   add one by hand.
4. **Agent**: the install command for a machine to watch.
5. **Modules**: tick the optional modules you want (wall mode, SLA reports,
   status pages, SLOs, threat map). All start off.
6. Three optional steps: **single sign-on and teams**, **on-call** (shown only
   while on-call schedules are on) and **retention**.

Every step can be skipped, and **Skip setup** ends the wizard at once. The
wizard is marked done on the server, so it does not come back in another
browser.

## API and backup

- `GET /api/instance` is public and answers only `setup_completed`.
- `GET /api/instance/settings` returns the name, language and time zone;
  `PUT /api/instance/settings` (administrators) changes them. An omitted field
  keeps its value. The former `product_mode` field is ignored if a client still
  sends it.
- The settings are part of the configuration backup (section `instance`). A
  restored backup never reopens the wizard on an instance where it was done, and
  a backup written by an older version (with `product_mode`) restores as is.
