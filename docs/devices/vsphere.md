# VMware vSphere

A vCenter Server and the hosts it manages, or a standalone ESXi host: hosts
that stop responding, are left in maintenance mode or report a red health,
virtual machines and their VMware Tools, datastores filling up or becoming
inaccessible, and the alarms vSphere itself raised.

## Network ports to open

From the DumbMonit server (or its relay agent) to the device:

| Port | Protocol | Purpose |
|---|---|---|
| 443 | TCP | vSphere API (HTTPS, default) |

DumbMonit reads vSphere with a dedicated user holding the built-in
**Read-only** role. It never powers a VM on or off, never enters maintenance
mode and never acknowledges an alarm.

The failure worth watching: **a datastore that fills up**. When a thin
provisioned datastore reaches 100%, every VM that needs to write to it is
paused at once. The alarms vCenter raises say it too, but only to whoever has
the client open.

## Why the Web Services API

DumbMonit talks to the vSphere Web Services API (SOAP, `/sdk`), not to the
REST API (`/api/vcenter/*`): a standalone ESXi host has no REST API, and on a
vCenter the REST API does not tell maintenance mode, the overall health
(`overallStatus`), the state of the VMware Tools or the triggered alarms. One
view of the whole inventory is read in a single pass, page by page, and
released at the end.

The session is kept from one probe to the next and reopened only when it
expires: vCenter does not log a new login every minute.

## What it watches

All metrics are prefixed `dumbmonit_vsphere_`. Health values follow vSphere's
`overallStatus`: 0 green, 1 yellow, 2 red, 3 gray (unknown).

| Metric | What | Labels |
|---|---|---|
| `info` | value 1 | `product` (`vcenter` or `esxi`), `version`, `build` |
| `hosts` | hosts by connection state: `connected`, `disconnected`, `notResponding` | `state` |
| `host_connection_state` | 0 connected, 1 disconnected by an administrator, 2 not responding | `esxi` |
| `host_power_state` | 0 powered on, 1 standby, 2 powered off, 3 unknown | `esxi` |
| `host_maintenance` | 1 in maintenance mode | `esxi` |
| `host_status` | overall health | `esxi` |
| `host_cpu_percent`, `host_memory_percent`, `host_memory_bytes`, `host_uptime_seconds` | connected hosts only | `esxi` |
| `vms` | VMs by power state: `poweredOn`, `poweredOff`, `suspended` (templates excluded) | `state` |
| `templates` | VM templates | |
| `vm_power_state` | 0 powered on, 1 powered off, 2 suspended; at most 1000 VMs | `vm` |
| `vm_status` | overall health | `vm` |
| `vm_tools_status` | powered-on VMs only: 0 up to date, 1 old version, 2 installed but not running, 3 not installed | `vm` |
| `vm_snapshots` | number of snapshots of the VM; only for a VM that has at least one | `vm` |
| `vm_snapshot_oldest_age_seconds` | age of the VM's oldest snapshot; only for a VM that has at least one | `vm` |
| `vm_consolidation_needed` | 1 when vSphere reports that the VM's disks need consolidation | `vm` |
| `vm_ip_info` | powered-on VMs only, always 1: the primary address VMware Tools reports (`guest.ipAddress`, read-only role is enough; none without Tools, loopback and link-local skipped). The map draws the monitors at that address under the VM | `vm`, `ip` |
| `datastores` | datastores | |
| `datastore_capacity_bytes`, `datastore_free_bytes`, `datastore_used_percent` | | `datastore` |
| `datastore_accessible` | 1 accessible, 0 inaccessible | `datastore` |
| `datastore_status` | overall health | `datastore` |
| `alarms` | triggered alarms not acknowledged, by color: `red`, `yellow` | `status` |
| `alarms_acknowledged` | triggered alarms already acknowledged | |
| `alarm` | one series per unacknowledged alarm, value = its health code, at most 64 | `alarm` (its name), `entity`, `entity_type`, `status` |
| `scrape_errors` | calls that failed on the last probe | |

Snapshots are read from each VM's snapshot tree, in the same pass as the rest of the inventory. DumbMonit keeps their number and the date of the oldest one, never their names. A VM without a snapshot has no snapshot series.

The [built-in rules](../alerting/rules.md#vmware-vsphere) that apply:

| Rule | What | Default threshold | Hold | Severity | Reminder |
|---|---|---|---|---|---|
| vSphere host not responding | A vSphere host is not responding to vCenter (`dumbmonit_vsphere_host_connection_state`: 2 not responding). A host disconnected on purpose (1) does not fire. | ≥ 2 | 3 min | Warning (`critical`) | 1 h |
| vSphere host health red | vSphere reports a host's overall health as red (`dumbmonit_vsphere_host_status == 2`). Gray means unknown and does not fire. | > 0 | 5 min | Warning (`critical`) | 1 h |
| vSphere host left in maintenance | A vSphere host has been in maintenance mode for a day: its VMs were moved away and it may have been forgotten there (`dumbmonit_vsphere_host_maintenance`). | > 0 | 24 h | Info (`info`) | 24 h |
| vSphere datastore almost full | A vSphere datastore is 90% full or more (`dumbmonit_vsphere_datastore_used_percent`); clears below 85 %. | ≥ 90 % | 15 min | Advisory (`warning`), escalates after 24 h | 6 h |
| vSphere datastore inaccessible | A vSphere datastore is inaccessible: the VMs stored on it cannot run (`dumbmonit_vsphere_datastore_accessible`). | < 1 | 5 min | Warning (`critical`) | 1 h |
| vSphere VM tools not running | VMware Tools are installed but not running in a powered-on VM: the guest may be hung (`dumbmonit_vsphere_vm_tools_status == 2`). VMs without Tools do not fire. | > 0 | 30 min | Info (`info`) | 24 h |
| vSphere red alarm | vSphere raised a red alarm that nobody acknowledged (`dumbmonit_vsphere_alarm{status="red"}`, one series per alarm, named after the alarm and the object). | > 0 | 5 min | Advisory (`warning`) | 6 h |
| vSphere VM snapshot too old | The oldest snapshot of a VM is more than three days old: snapshots grow, slow the VM down and can fill the datastore. VMware recommends keeping none beyond 72 hours (`dumbmonit_vsphere_vm_snapshot_oldest_age_seconds`). | > 3 d | 1 h | Advisory (`warning`) | 24 h |
| vSphere VM disks need consolidation | vSphere reports that the disks of a VM need consolidation: a snapshot removal left delta files behind, and they keep growing (`dumbmonit_vsphere_vm_consolidation_needed`). | > 0 | 1 h | Advisory (`warning`) | 24 h |

## The monitor page

The panel above the charts reads what the probe stored; opening the page never
queries vSphere. It says what is wrong first (hosts not responding, red
alarms, datastores almost full or inaccessible), then the hosts with their
load, the datastores with their fill level, and the VMs by power state.

## How it was validated

Against the govmomi simulator `vcsim` 0.56.0, in vCenter and in ESXi mode:
login and refused password, session reuse and expiry, paging through the
inventory, a host not responding, a host in maintenance mode, a powered-off VM
and a template. The simulator raises no alarm: reading the triggered alarms is
validated against the documented `AlarmState` structure of the Web Services
API.

## Create a read-only user in vSphere

1. On a vCenter, open Administration → Single Sign On → Users and Groups → Users, choose the domain vsphere.local, click Add and enter this user name with a long random password.

    ```
    dumbmonit
    ```

2. Open Administration → Access Control → Global Permissions → Add: pick that user, role Read-only, tick Propagate to children.

3. On a standalone ESXi host instead: in the host client, Manage → Security & users → Users → Add user dumbmonit, then Host → Actions → Permissions → Add user with role Read-only.

4. In DumbMonit, enter the vCenter or host address, with the user dumbmonit@vsphere.local on a vCenter or dumbmonit on a host. Add only the vCenter when one manages the hosts.

### Required permissions

- **Read-only, propagated to children**: Sees hosts, VMs, datastores and alarms

!!! warning
    vCenter and ESXi use a self-signed certificate by default: enable "Accept an unverifiable certificate" unless the DumbMonit host trusts their certificate authority.

Vendor documentation: [vSphere global permissions](https://docs.vmware.com/en/VMware-vSphere/8.0/vsphere-security/GUID-74F53189-EF41-4AC1-A78E-D25621855800.html)

### Why these permissions

Do not reuse the single sign-on domain account or the host's built-in
account: the `Read-only` role is all DumbMonit needs. It sees every host, VM,
datastore and alarm, and can change nothing.

When a vCenter manages the hosts, add only the vCenter: it reports every host,
VM and datastore.

DumbMonit only reads, through the vSphere Web Services API (`/sdk`). It never
powers a VM on or off, never enters maintenance mode and never acknowledges an
alarm.

## Credentials

| Credential | Fields |
|---|---|
| User name / password | The dedicated user with the Read-only role: `dumbmonit@vsphere.local` on a vCenter, `dumbmonit` on a host. |

Address: a host name or IP (`vcenter.lan`), `host:port`, or a full URL; a
trailing `/sdk` or `/ui` is accepted. Options: port (443), certificate check,
request timeout (20 s) and triggered alarms (read).

## Known limits

- License expiry is not read: the Read-only role does not see licenses.
- At most 1000 VMs and 64 alarms are described one by one.
- No performance counters per VM (processor, memory, disk latency): DumbMonit reads the inventory and its states.
- Fans, power supplies and temperatures are only seen through the overall health of a host and the alarms vSphere raises.
- Snapshots and consolidation were built from the documented structure of the API and have not been checked on a real vCenter yet.
