# Network flows (beta)

Routers, firewalls and switches can tell who talks to whom: they export
**flow records** (NetFlow v5/v9, IPFIX) or **packet samples** (sFlow v5).
DumbMonit receives them on UDP, keeps the largest exchanges of each minute and
shows them on the device page, in a **Traffic** section: top talkers, top
ports and top conversations over the last hour, 6 hours, 24 hours or 7 days.

This is a beta feature, off by default. It answers "what is eating the
uplink right now?", not "who visited which site last month": see
[Limits](#limits).

## Turn it on

1. Add the exporting router or switch as a device (any kind: SNMP, ping,
   MikroTik, OPNsense…). **Only flows sent from the address of a monitored
   device are kept**; the others are counted as "unknown sender" and dropped.
2. In **Settings → System → Network flows**, turn on **Receive flows**. The
   default ports are 2055/UDP for NetFlow v5/v9 and IPFIX, and 6343/UDP for
   sFlow v5.
3. Publish the ports of the container, in `docker-compose.yml`:

   ```yaml
   services:
     dumbmonit:
       ports:
         - "2055:2055/udp"
         - "6343:6343/udp"
   ```

4. Point the exporter to the address of the DumbMonit host (examples below).

The same section shows whether each receiver listens, the datagrams received
and dropped since the server started, the devices that export flows and the
last unknown senders. A router that shows up there as an unknown sender sends
from another address than the one of its device: use that address for the
device, or set the source address of the export on the router.

!!! note "Source address behind Docker"
    The sender is recognised by the source address of its datagrams. Docker
    keeps it for published ports in the default setup. If every exporter shows
    up as the Docker gateway (`172.17.0.1` or similar), the userland proxy is
    in the way: run the container with `network_mode: host`, or set
    `"userland-proxy": false` in `/etc/docker/daemon.json`.

`DUMBMONIT_FLOWS_BIND` limits the receivers to one local address
(`0.0.0.0` by default).

## Network ports to open

From the exporters to the DumbMonit server:

| Port | Protocol | Purpose |
|---|---|---|
| 2055 | UDP | NetFlow v5, v9 and IPFIX (configurable) |
| 6343 | UDP | sFlow v5 (configurable) |

Keep these ports on the local network: flow datagrams are neither
authenticated nor encrypted.

## Exporter examples

In every example, `192.168.1.10` is the DumbMonit host.

### MikroTik RouterOS 7

```
/ip/traffic-flow/set enabled=yes interfaces=all active-flow-timeout=1m inactive-flow-timeout=15s
/ip/traffic-flow/target/add dst-address=192.168.1.10 port=2055 version=ipfix
```

`version=9` works too. Set `src-address=` on the target when the router has
several addresses and the device in DumbMonit uses another one.

### OPNsense

**Reporting → NetFlow**: choose the listening interfaces (LAN, and WAN if you
want the Internet side), version **v9**, and the destination
`192.168.1.10:2055`. Apply.

### pfSense, or any Linux box, with softflowd

On pfSense, install the **softflowd** package, then in **Services →
softflowd**: the interface to watch, host `192.168.1.10`, port `2055`, NetFlow
version **9** (or IPFIX), flow tracking level **Full**, and a maximum flow
lifetime of 60 seconds. On Linux:

```
softflowd -i eth0 -n 192.168.1.10:2055 -v 9 -t maxlife=60
```

### Cisco IOS / IOS XE (Flexible NetFlow)

```
flow exporter DUMBMONIT
 destination 192.168.1.10
 source GigabitEthernet0/0
 transport udp 2055
 export-protocol netflow-v9
!
flow monitor DUMBMONIT
 exporter DUMBMONIT
 cache timeout active 60
 record netflow ipv4 original-input
!
interface GigabitEthernet0/1
 ip flow monitor DUMBMONIT input
```

The `source` interface must carry the address of the device in DumbMonit.

### Ubiquiti

EdgeRouter (EdgeOS):

```
configure
set system flow-accounting interface eth1
set system flow-accounting netflow server 192.168.1.10 port 2055
set system flow-accounting netflow version 9
set system flow-accounting netflow timeout expiry-interval 60
commit ; save
```

Recent UniFi Network releases can export NetFlow (IPFIX) from a UniFi gateway:
look for NetFlow in the gateway settings and enter `192.168.1.10`, port
`2055`. Menu names change between releases.

### sFlow switches

Set the collector to `192.168.1.10`, UDP port `6343`, and a sampling rate
suited to the link speed (1 in 1000 or more on gigabit links). DumbMonit
reads flow samples (raw packet headers of Ethernet, IPv4 and IPv6 frames, and
the sampled IPv4/IPv6 records) and ignores counter samples: interfaces are
already watched by SNMP.

## What it records

Each exporter is aggregated per minute by source address, destination
address, IP protocol and port. The **port** is the lower of the two ports of
the exchange, usually the service (443 for HTTPS whichever way the bytes
flow); it is 0 for protocols without ports (ICMP, GRE…). Two series are
written, with the labels `target` (the device id), `src`, `dst`, `proto`
(`tcp`, `udp`, `icmp`… or the protocol number) and `port`:

| Metric | What |
|---|---|
| `dumbmonit_flow_bytes_total` | Bytes of the exchange during the minute |
| `dumbmonit_flow_packets_total` | Packets of the exchange during the minute |

Each point is the volume of its minute, not a running counter:
`sum_over_time(dumbmonit_flow_bytes_total{target="3"}[1h])` gives the volume
of the last hour. Flows are kept as long as the other metrics (the raw
retention of the metrics database, see
[Data & retention](../using/settings.md#data-retention)); they
are not part of the long-term history.

## Limits

- **Top 50 per minute.** For each exporter, only the 50 largest exchanges of
  each minute become series; the rest is added to one series whose labels are
  all `other`. A global cap of 10,000 distinct series per hour sends further
  new exchanges to `other` as well (counted as "folded into others").
- **Sampling.** NetFlow v5 and sFlow carry their sampling rate, and volumes are
  multiplied by it, as are NetFlow v9 / IPFIX records that include their
  sampling interval. A rate announced only in an options template is not
  applied: export unsampled, or read the figures as relative.
- **Minute of arrival.** A flow counts in the minute it is received, not in
  the minute it started: keep the active timeout of the exporter at about
  60 seconds so long transfers are spread over time.
- **Templates.** NetFlow v9 and IPFIX data waits for its template, which
  exporters send every few minutes; data received before is counted as
  "waiting for a template". At most 256 templates per exporter are kept, and a
  template not refreshed for 30 minutes is forgotten.
- **Not inspected.** No payload, no host names, no applications: addresses,
  protocol, port and volume only.
- **On the server only.** Relay agents do not receive flows yet.
