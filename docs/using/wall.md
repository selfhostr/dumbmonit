# Wall mode

!!! note "Optional module"
    Wall mode is an optional module, off on a new installation: switch it on in *Settings → System → Modules* ([Settings](settings.md#modules)).

`/wall` shows the console's own board, full screen, for a monitor that stays on in
a room. It refreshes by itself and keeps the screen awake.

From the top:

- the **verdict** in a few words ("All good", "3 problems") with the date and
  the clock, and when the data was last updated;
- the **outside weather**, to the right of the date once a city is chosen: a small
  icon and the temperature, nothing else ([Weather](#weather));
- the **open alerts**, most severe first;
- a quiet strip of **three figures** along the foot: monitors up, availability
  over 30 days and open alerts.

There are no widgets: black ground, the console's own type, states written next
to a shape and never by colour alone.

## Opening and leaving

Open it with the **Wall mode** icon at the bottom of the left menu, from the command palette
("Wall mode") or by typing the URL. ++esc++ or **Exit** returns to the console.
An account limited to some teams or clients does not get the wall, since it
shows the whole fleet.

## A screen that only shows the wall {#wall-display-account}

For a TV or a monitor left on in a room, create an account with the **Wall
display** role (**Settings → Team & access → Users**) and sign in with it on
that screen. It lands on the wall right after signing in, and stays there:

- every other address leads back to `/wall`, with the options of its address
  (`?city=`…) kept from the sign-in link;
- **Exit** becomes **Sign out**, which takes two taps, and ++esc++ does nothing;
  fullscreen, the light board, the backdrop, the beep still
  work, since they belong to the screen;
- device tiles and problems are not links;
- the server answers only what the wall reads, and `403` to everything else.

**Settings → Wall display** (clock, beep, rotation) is out of its reach. Those
options belong to the browser, not the account: set them on that screen while
signed in with another account, then sign in as the wall display.

The session is renewed as long as the screen shows the wall, so nobody has to
type the password again; it ends after `DUMBMONIT_SESSION_DAYS` without use,
when the account is disabled, its password reset or its role changed. The
two-factor requirement does not apply to it. Unlike other accounts limited to
some teams, a wall display limited by its teams does get the wall, showing
only the monitors those teams grant. See
[Wall display accounts](settings.md#wall-display-accounts).

## Keys, options and links

- ++f++ toggles fullscreen (a discreet button next to Exit does the same) and
  ++t++ switches between the dark wall and a plain light board. The pointer
  disappears after 3 seconds without a move and comes back with the next one.
- **Settings → Wall display** sets, for this browser, the clock's format (24 h or
  12 h) and whether it shows seconds, an optional short **beep when a new
  critical problem appears** (off by default; browsers only allow sound after a
  click or key press on the wall), and an optional **rotation of the monitors**: when there are more
  monitors than fit on the screen, the tiles change page every 15, 30 or 60
  seconds instead of ending with "and N more" (off by default; problems come
  first).
- A line under the verdict announces the incident or maintenance published on a
  status page that is in progress, or a maintenance window covering now.
- `?theme=dark|light` and `?scene=paris|none` (the same as `?city=`) configure
  one display from its address without changing what is remembered. `?city=`
  and `?weather=` work as described below.

## Backdrop

The wall shares the console's dark look. The **Backdrop** button (top right,
next to Exit) can draw Paris behind the verdict and the clock, or leave a plain
board (*None*). Paris is the default and the choice is remembered per browser;
`?city=paris` (or `none`) on a link chooses it for that display only. A saved or
linked choice for a retired backdrop (New York, Tokyo, Dubai, *Auto*) lands on
Paris. The sky follows the board: stars when all is well, clouds on an advisory,
a storm on a warning. On a phone, or while the tab is hidden, the city holds
still.

## Weather {#weather}

Beside the date, a small icon (sun, clouds, rain, snow, thunderstorm, fog; the
moon at night) and the outside temperature, such as "12°". Nothing else: no
forecast, no wind. The icon has a written label for screen readers.

It is off until an administrator picks a city in **Settings → Wall display →
Wall weather**: search by name, then choose **°C** or **°F**. The DumbMonit
server asks [Open-Meteo](https://open-meteo.com/) (free, no key) and keeps the
answer for ten minutes, whatever the number of walls; browsers never contact it.
Only the city's coordinates are sent. Without a city nothing is requested, and
if the service does not answer the wall simply shows no icon.
