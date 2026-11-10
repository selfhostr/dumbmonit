# Unraid

An Unraid server through its GraphQL API: an array that is not started,
disks disabled, missing or reporting read errors, parity checks that found
errors or have not run for weeks, cache pools filling up, containers that
should be running and are not, and the state of the VMs.

## Network ports to open

From the DumbMonit server (or its relay agent) to the device:

| Port | Protocol | Purpose |
|---|---|---|
| 80 | TCP | Web UI and GraphQL API (HTTP, default) |
| 443 | TCP | Only with the HTTPS protocol option |

DumbMonit sends an API key with the Viewer role in the `x-api-key` header to
`/graphql`. The Viewer role reads everything and can change nothing. The API
is built into Unraid from 7.2; on 6.12 to 7.1 it comes with the Unraid Connect
plugin.

Every minute DumbMonit sends one required query (version, array, parity, data
and cache disks) and four optional ones, each on its own so that a field an
older API version lacks, or VMs turned off, only leaves out that part: the
parity check, the Docker containers, the VMs and the unread notifications.

!!! warning "Validated against the vendor documentation only — not yet tested on a real system"

    This integration was built from Unraid's API documentation and the
    published GraphQL schema of the Unraid API (field names, enumerations,
    sizes in kilobytes), not against a running server. Every query is a
    read. Tell us what breaks.

## What it watches

All metrics are prefixed `dumbmonit_unraid_`.

| Metric | Labels | Meaning |
|---|---|---|
| `version_info` | `version` | Value 1: the Unraid version. |
| `array_started` | | 1 when the array is started. |
| `array_state_info` | `state` | Value 1: `started`, `stopped`, `new_array`, `recon_disk`, `disable_disk`… |
| `array_size_bytes`, `array_used_bytes`, `array_free_bytes`, `array_used_percent` | | Array capacity. |
| `disk_ok` | `disk`, `role` (`parity`, `data`, `cache`) | 1 when the disk's status is `DISK_OK`. Empty slots are left out. |
| `disk_status_info` | `disk`, `role`, `status` | Value 1: Unraid's status (`disk_ok`, `disk_dsbl`, `disk_np_missing`, `disk_invalid`…). |
| `disk_errors` | `disk`, `role` | Read errors since the array started (the Errors column). |
| `disk_temperature_celsius` | `disk`, `role` | Only while the disk spins: Unraid reports none for a sleeping disk. |
| `disk_spinning` | `disk`, `role` | 1 when the disk spins. |
| `disk_size_bytes`, `disk_fs_size_bytes`, `disk_used_percent` | `disk`, `role` | Size, file system size and fill, for disks with a file system. |
| `disks`, `disks_problem`, `disks_with_errors` | | Counts. |
| `parity_check_status_info` | `status` | Value 1: `completed`, `running`, `paused`, `cancelled`, `failed`, `never_run`. |
| `parity_check_running`, `parity_check_progress_percent` | | A check under way. |
| `parity_check_ok` | | After a finished check: 1 without sync errors, 0 with errors or when it failed. A cancelled check proves nothing and sets nothing. |
| `parity_check_errors`, `parity_check_age_seconds`, `parity_check_duration_seconds` | | The last check. |
| `containers`, `containers_running`, `containers_autostart_stopped` | | Docker containers. |
| `container_running`, `container_autostart_stopped` | `container` | Per container; the second is 1 when a container set to start with the array is not running. |
| `vms`, `vms_running` | | VMs. |
| `vm_state_info`, `vm_crashed` | `vm` (and `state`) | Per VM. |
| `notifications_unread` | `importance`: `alert`, `warning`, `info` | Unread notifications. |
| `registration_ok` | | 1 when Unraid reports no license problem, 0 when the license state is an error (expired trial, key that does not match the flash drive, missing key file…). |
| `registration_info` | `state`, `type` | Value 1: the license state as Unraid reports it (`trial`, `pro`, `eexpired`, `eguid`…) and the license type. |
| `trial_expiry_seconds` | | Until the trial license ends; only during a trial. |
| `os_updates_expiry_seconds` | | Until the license stops being eligible for Unraid OS updates; absent when the license has no such date. |
| `uptime_seconds` | | Since the server started. |
| `cpu_usage_percent` | | Processor load, all cores together. |
| `memory_total_bytes`, `memory_available_bytes`, `memory_used_percent` | | Memory. What counts as used is what is not available: the cache Unraid can give back is not counted. |
| `swap_used_percent` | | Only when the server has swap. |

The license and the system load (uptime, processor, memory) come from two more optional queries. An older API version that does not know them only leaves those rows out: the rest of the monitor is unaffected.

The [built-in rules](../alerting/rules.md#unraid) that apply:

| Rule | What | Default threshold | Hold | Severity | Reminder |
|---|---|---|---|---|---|
| Unraid array stopped | The Unraid array is not started: shares, containers and VMs that live on it are unavailable (`dumbmonit_unraid_array_started`). | < 1 | 10 min | Warning (`critical`) | 6 h |
| Unraid disk disabled or missing | An Unraid disk is disabled, missing or invalid: the array runs degraded, and one more failure loses data (`dumbmonit_unraid_disk_ok`). | < 1 | 5 min | Warning (`critical`) | 6 h |
| Unraid disk read errors | An Unraid disk has reported read errors since the array started: it may be disabled at the next one (`dumbmonit_unraid_disk_errors`). | > 0 | 5 min | Advisory (`warning`) | 1 d |
| Unraid disk too hot | An Unraid array or parity disk has stayed above 55 °C for fifteen minutes (`dumbmonit_unraid_disk_temperature_celsius{role=~"data\|parity"}`). | > 55 °C | 15 min | Advisory (`warning`) | 6 h |
| Unraid parity check found errors | The last Unraid parity check failed or found sync errors: parity no longer matches the data (`dumbmonit_unraid_parity_check_ok`). | < 1 | 5 min | Advisory (`warning`) | 1 d |
| Unraid parity check overdue | The last Unraid parity check is more than 40 days old: a monthly check was missed (`dumbmonit_unraid_parity_check_age_seconds`). | > 40 d | 1 h | Advisory (`warning`) | 1 d |
| Unraid cache nearly full | An Unraid cache pool is more than 90 % full: writes fall back to the slow array, or fail (`dumbmonit_unraid_disk_used_percent{role="cache"}`). | > 90 % | 30 min | Advisory (`warning`) | 1 d |
| Unraid container stopped | A Docker container set to start with the array is not running (`dumbmonit_unraid_container_autostart_stopped`). | > 0 | 10 min | Advisory (`warning`) | 6 h |
| Unraid VM crashed | Unraid reports a virtual machine as crashed (`dumbmonit_unraid_vm_crashed`). | > 0 | 5 min | Advisory (`warning`) | 6 h |
| Unraid alert unread | Unraid raised a notification of the highest importance (alert) that nobody has read: failed disk, overheating, array error… (`dumbmonit_unraid_notifications_unread{importance="alert"}`). | > 0 | 10 min | Advisory (`warning`) | 24 h |
| Unraid array nearly full | The array is more than 95 % full: writes to the shares will soon fail. Clears below 93 % (`dumbmonit_unraid_array_used_percent`). | > 95 % | 1 h | Advisory (`warning`) | 24 h |
| Unraid memory nearly exhausted | More than 95 % of the memory has been in use for fifteen minutes: Unraid runs from memory, and containers or VMs are killed when it runs out. Clears below 90 % (`dumbmonit_unraid_memory_used_percent`). | > 95 % | 15 min | Advisory (`warning`) | 6 h |
| Unraid license problem | Unraid reports a license problem (expired trial, key that does not match the flash drive, missing key file…): the array may refuse to start after the next stop (`dumbmonit_unraid_registration_ok`). | < 1 | 30 min | Advisory (`warning`) | 24 h |
| Unraid trial ending | The trial license ends in less than three days: after it, the array no longer starts (`dumbmonit_unraid_trial_expiry_seconds`). | < 3 d | 1 h | Advisory (`warning`) | 24 h |
| Unraid OS updates eligibility ending | The license stops being eligible for Unraid OS updates in less than 30 days. The server keeps working; it only stops receiving new versions (`dumbmonit_unraid_os_updates_expiry_seconds`). | < 30 d | 1 h | Info (`info`) | 7 d |

## The monitor page

The Array panel says what is wrong in a sentence, shows the array fill, the
last parity check, the running containers and VMs, then lists every disk —
parity first, then data, then cache — with its status, temperature and fill,
and the containers that should be running.

## Create a read-only API key for DumbMonit

1. On Unraid 6.12 to 7.1, install the Unraid Connect plugin from the Apps tab first. Unraid 7.2 or later has the API built in.
2. Open Settings → Management Access → API Keys and create a key named as follows, with the Viewer role.

    ```
    dumbmonit
    ```

3. Copy the key: Unraid shows it only once.
4. In DumbMonit, enter the Unraid address (for example tower.lan), paste the key as API key and choose https if the web interface is served over HTTPS.

### Required permissions

- **Viewer role** — Reads everything, changes nothing

!!! warning
    Unraid reports no temperature for a disk that is spun down, and DumbMonit never wakes one: a sleeping disk simply has no temperature until it spins up.

Vendor documentation: [Using the Unraid API](https://docs.unraid.net/API/how-to-use-the-api/).

### Why these permissions

The Viewer role reads everything and can change nothing. No Unraid Connect
sign-in is needed for local use.

## Credentials

| Credential | Fields |
|---|---|
| API key | A key with the Viewer role, sent as `x-api-key`. |

Address: the host name or IP of the Unraid server, `host:port`, or a URL.
Options:

- **Protocol** (`http`) and **Port** (80): those of the web interface. With
  SSL set to Strict under Settings > Management Access, use `https`, and the
  `myunraid.net` address if the certificate is that one.
- **Accept an unverifiable certificate**: for a self-signed certificate.
- **Timeout per request** (15 s).

## Known limits

- The UPS is not read: connect it through [NUT](nut.md) instead.
- No SMART detail: DumbMonit reads the status, the errors and the temperature Unraid gives for each disk, not the SMART attributes.
- Whether a container has an image update waiting is not read.
- The license, processor and memory figures need a recent version of the Unraid API. On a version that lacks them, those rows and their rules are simply absent.

## Troubleshooting

"Unraid refused the API key (401)": the key is wrong or was deleted.

"Unraid accepted the API key but refused to answer (403)": the key has no
role, or a role narrower than Viewer.

No container, VM or parity figure: that optional query failed (VMs turned off,
an older API version without the field); the rest is still read. The reason is
in the server log at debug level.
