# UniFi Network

The UniFi network and what it manages: gateways, switches and access points
that are offline, isolated or waiting for adoption, firmware updates waiting,
the WAN links and the Internet connection as the gateway sees them, the number
of clients, and the alarms UniFi raised. It works with the UniFi Network
application of a UniFi OS console (Dream Machine, Cloud Gateway, Cloud Key)
and with a self-hosted UniFi Network Server.

DumbMonit reads UniFi with an API key or with a local account holding the
**View Only** role. It never restarts, adopts, upgrades or provisions a device.

The failure worth watching: **an access point or a switch drops off quietly**.
Clients roam to the next access point, the network keeps working, and the
coverage hole or the lost uplink is only noticed days later. The same goes for
a second WAN link that went down: nothing breaks until the first one fails too.

## Network ports to open

From the DumbMonit server (or its relay agent) to the device:

| Port | Protocol | Purpose |
|---|---|---|
| 443 | TCP | API (HTTPS, default) |
| 8443 | TCP | Only with a self-hosted UniFi Network Server |

## Two ways in

| Credential | API used | What it reads |
|---|---|---|
| API key (UniFi OS console) | The official Integration API, at `/proxy/network/integration/v1` on a console and `/integration/v1` on a self-hosted server | Version, devices and their state, firmware updates, load, memory and uptime of each online device, number of clients |
| View Only account | The classic API (`/api/s/<site>/stat/*`), at `/proxy/network/api/…` on a console | All of the above, plus clients per device and by type, the WAN links, the WAN and Internet status, and alarms |

The WAN, Internet and alarm status only exist in the classic API. With a key,
DumbMonit also asks the classic API for the WAN and Internet status with the
same key; if the controller refuses it, those series are simply absent, without
an error.

Alarms are the entries of UniFi's "critical" system log over the last 24 hours
(UniFi Network 8 and later), or the unarchived alarms on older versions. That is
the only `POST` DumbMonit sends, and it only reads.

A session opened with the View Only account is kept from one probe to the next
and reopened when it expires: UniFi does not log a new login every minute.

## What it watches

All metrics are prefixed `dumbmonit_unifi_`. Device series carry `device` (its
name, or its MAC address when it has none), `mac`, `model` and `type`
(`gateway`, `switch`, `access_point`, `other`).

| Metric | What | Labels |
|---|---|---|
| `info` | value 1 | `version`, `api` (`integration` or `classic`) |
| `devices` | devices by state: `online`, `offline`, `pending`, `updating`, `adopting`, `adoption_failed`, `isolated`, `other` | `state` |
| `devices_upgradable` | adopted devices with a firmware update available | |
| `device_state` | 0 online, 1 offline, 2 pending adoption, 3 updating or provisioning, 4 adopting, 5 adoption failed, 6 isolated, 7 other | device labels |
| `device_up` | 1 online or updating, 0 offline, isolated or failed to adopt; absent for a device not adopted yet | device labels |
| `device_upgradable` | 1 when a firmware update is available | device labels, `firmware` |
| `device_uptime_seconds`, `device_cpu_percent`, `device_memory_percent` | online devices only | device labels |
| `device_clients` | clients connected to the device (View Only account) | device labels |
| `wan_link_up` | 1 when a WAN port of the gateway is up; a disabled port has no series | device labels, `wan` (`wan1`, `wan2`) |
| `wan_up`, `internet_up` | the gateway's WAN and Internet status; absent without a UniFi gateway | |
| `internet_latency_seconds` | latency measured by the gateway | |
| `wan_availability_percent` | availability of each WAN over the last 24 hours | `wan` |
| `subsystem_status` | 0 ok, 1 warning, 2 error, 3 unknown, for `wlan`, `wan`, `www`, `lan`, `vpn` | `subsystem` |
| `clients` | clients on the site | |
| `clients_by_type` | `wireless`, `wired`, `guest`, `vpn` (View Only account) | `type` |
| `alarms` | critical system log entries of the last 24 hours, or unarchived alarms | |
| `scrape_errors` | calls that failed on the last probe | |
| `port_enabled`, `port_up`, `port_speed_bps`, `port_full_duplex`, `port_uplink` | ports of each online switch or gateway (View Only account, `port_table`); `port_up` is absent for a disabled port | `device`, `mac`, `port` (front-panel number), `port_name` |
| `port_poe_enabled`, `port_poe_watts` | PoE of each port | same |
| `port_rx_bytes_total`, `port_tx_bytes_total`, `port_rx_errors_total`, `port_tx_errors_total` | per-port counters, drawn by the switch panel | same |
| `port_neighbor_info` | value 1, LLDP neighbour of the port (`lldp_table`) | same, `remote_chassis`, `remote_port` |

The [built-in rules](../alerting/rules.md#unifi-network) that apply:

| Rule | What | Default threshold | Hold | Severity | Reminder |
|---|---|---|---|---|---|
| UniFi device offline | A UniFi device adopted by the controller is offline, isolated or failed to adopt (`dumbmonit_unifi_device_up`). | < 1 | 5 min | Advisory (`warning`), escalates after 1 h | 6 h |
| UniFi Internet down | The UniFi gateway reports no Internet connection (`dumbmonit_unifi_internet_up`). Without a UniFi gateway the series does not exist and the rule stays silent. | < 1 | 3 min | Warning (`critical`) | 1 h |
| UniFi WAN link down | A WAN link of the UniFi gateway is down (`dumbmonit_unifi_wan_link_up`): with two links, redundancy is lost while nothing seems broken. A disabled port has no series. | < 1 | 5 min | Advisory (`warning`) | 6 h |
| UniFi gateway CPU high | The UniFi gateway's processor has been above 90% for fifteen minutes (`dumbmonit_unifi_device_cpu_percent{type="gateway"}`); clears below 80 %. | > 90 % | 15 min | Advisory (`warning`) | 6 h |
| UniFi device waiting for adoption | A device appeared on the network and waits to be adopted in UniFi (`dumbmonit_unifi_devices{state="pending"}`). | > 0 | 30 min | Info (`info`) | 24 h |
| UniFi firmware update available | A firmware update is available for UniFi devices (`dumbmonit_unifi_devices_upgradable`). | > 0 | 1 h | Info (`info`) | 7 d |
| UniFi alarms | UniFi logged critical events or raised alarms in the last 24 hours (`dumbmonit_unifi_alarms`). Only read with a View Only account. | > 0 | 5 min | Advisory (`warning`) | 24 h |
| UniFi subsystem in error | A UniFi subsystem (Wi-Fi, LAN or VPN) reports an error in the controller's health view. Internet and the WAN links have their own rules (`dumbmonit_unifi_subsystem_status`: 2). | ≥ 2 | 10 min | Advisory (`warning`) | 6 h |
| UniFi device overheating | A UniFi device reports that it is overheating (`dumbmonit_unifi_device_overheating`). | > 0 | 5 min | Advisory (`warning`) | 6 h |
| UniFi PoE budget nearly used | A switch draws more than 90 % of its PoE budget: the next device plugged in may not be powered. Clears below 80 % (`dumbmonit_unifi_device_poe_used_watts` / `dumbmonit_unifi_device_poe_budget_watts`). | > 90 % | 15 min | Advisory (`warning`) | 24 h |
| UniFi Wi-Fi channel saturated | The channel of an access point radio has been busy more than 80 % of the time for thirty minutes: Wi-Fi is slow for everyone on it. Clears below 65 % (`dumbmonit_unifi_radio_channel_utilization_percent`). | > 80 % | 30 min | Advisory (`warning`) | 24 h |
| UniFi Wi-Fi clients with a weak signal | More than 40 % of the Wi-Fi clients, with at least five connected, have a signal weaker than -75 dBm: the access points are badly placed or too few (`dumbmonit_unifi_wifi_clients_weak_signal` / `dumbmonit_unifi_wifi_clients`). | > 40 % | 1 h | Info (`info`) | 7 d |
| UniFi port errors | A switch port logged more than 100 receive or transmit errors in fifteen minutes: check the cable and the connectors (`dumbmonit_unifi_port_rx_errors_total` + `dumbmonit_unifi_port_tx_errors_total`). | > 100 | 15 min | Advisory (`warning`) | 24 h |

## Devices

`/rest/user` (the classic API, read with either credential) lists every
client the controller has ever seen — most of a home network, in practice:
phones passing by, guest Wi-Fi, smart devices that never come back. Tracking
all of them for staleness would bury the alert under devices that simply
never return, not devices that went silent. By default only clients the user
marked one way or another count: named in UniFi, reserved on a fixed IP, or
noted. The **Clients to watch besides the marked ones** option adds specific
MAC addresses or client names, or `all` to track every client (not
recommended on anything but a small, mostly-static network).

Tracked clients are published under the generic family shared by every
DumbMonit integration that tracks client devices — `dumbmonit_client_device_*`,
prefixed without `unifi`, documented in full under
[Client devices](../alerting/rules.md#client-devices):

| Metric | What | Labels |
|---|---|---|
| `last_seen_timestamp_seconds` | Last connection, as `/rest/user` reports it | `device`, `type` (`Wired`, `Wireless` or `Unknown`), `os` (empty), `user` (empty), `kind` |
| `stale_seconds` | Positive once a tracked client has gone past `device_stale_days` (3 by default) without connecting; drives the [Client device stale](../alerting/rules.md#client-devices) rule | `device`, `type`, `os`, `user`, `kind`, `signal` (always `connection`: UniFi has no backup to report) |

## Going deeper

On top of the device states, WAN and clients above, DumbMonit reads, with a local account or an API key that the classic API accepts:

- **Device health**: satisfaction score, temperature and overheating flag of every online device.
- **PoE**: the PoE budget of each switch and what its ports draw.
- **Wi-Fi**: channel utilization and client count of every radio (2.4, 5 and 6 GHz), the number of Wi-Fi clients and how many have a signal weaker than -75 dBm.
- **Gateway speed test**: download, upload and latency of the last test run by the gateway.

Nothing here sends a command; if the classic API refuses a call, the matching series are simply missing and the monitor is not in error.

## The monitor page

The panel above the charts reads what the probe stored; opening the page never
queries UniFi. It says what is wrong first (devices offline, Internet or a WAN
link down, alarms), then the devices by type with their state, firmware and
load, and the clients, then the table of tracked clients with their last
connection.

## How it was validated

Login, the View Only role, error answers (refused password, unknown site,
refused key, expired session) and the answers of a controller without devices
were checked against UniFi Network 10.6.106, self-hosted. No device could be
adopted there: the device list, the health of an equipped site, the known
clients (`/rest/user`) and the Integration API payloads are validated against
the official documentation.

## Create a read-only access in UniFi Network

1. For an API key on a UniFi OS console: UniFi Network → Settings → Control Plane → Integrations → Create API Key. Name it DumbMonit and copy it: it is shown once.

2. Or create a local account (needed for WAN and alarm status, and on a self-hosted server): Settings → Admins & Users → Create New, local access only.

    ```
    dumbmonit
    ```

3. Set that account's UniFi Network role to View Only.

4. In DumbMonit, enter the address (unifi.lan, or unifi.lan:8443 for a self-hosted server) and paste the key or the account.

5. If the network is not the default site, set the Site option to the short name shown after /manage/ or /network/ in the address bar.

### Required permissions

- **View Only (UniFi Network role)** — Reads everything and changes nothing

!!! warning
    Use a local account, never the owner or a UI.com cloud account, and accept the console's self-signed certificate in the options unless you installed your own.

### Notes

Keep two-factor authentication off for this local View Only account only, since
a monitoring server cannot type a code. A leaked owner or UI.com password would
open every console linked to it.

UniFi knows every client that ever connected, most of them phones passing by:
only those named, reserved on a fixed IP, or noted in UniFi are watched for
staleness by default, to avoid an alert for every visitor's phone. List more in
the Clients to watch option, or enter all.

## Controller not set up yet

A self-hosted controller whose first-run setup wizard is not finished answers every request with a redirect to `/setup`. DumbMonit then reports "The UniFi controller is not set up yet" as a configuration error (not a refused password): finish the wizard in the browser, then probe again.

## Credentials

| Credential | Fields |
|---|---|
| API key (UniFi OS console) | A key created in Settings → Control Plane → Integrations, sent as `X-API-KEY`. |
| View Only account | The user name and password of the local View Only account. |

Address: a host name or IP (`unifi.lan`), `host:port` (`unifi.lan:8443` for a
self-hosted server), or a full URL; an address copied from the browser, with
`/manage/…` or `/network/…`, is accepted. Options: site (`default`), port (443),
certificate check, request timeout (15 s), clients to watch besides the marked
ones (empty, or `all`), and device staleness (`device_stale_days`, 3 days by
default).

## Known limits

- The Internet and WAN status, the alarms, PoE and Wi-Fi details only exist in the classic API. Some controllers refuse it to an API key: those series are then absent, without an error. A View Only account reads everything.
- Alarms are the critical entries of the system log over the last 24 hours on UniFi Network 8 and later, the unarchived alarms before that.
- Only the clients you marked in UniFi (named, with a fixed IP, or annotated) are followed one by one, unless the `watched_clients` option says otherwise.
- No command is ever sent: no restart, no adoption, no firmware upgrade.
