# Prometheus exporters

Anything that serves the Prometheus text format on an HTTP `/metrics` page can be
watched by DumbMonit, without Prometheus. Add a **Prometheus exporter** monitor
(kind `prometheus`): on every check DumbMonit reads the page and stores each
series as `dumbmonit_<name>`, so it appears in the metrics explorer and can be
used in alert rules. All options are described on the
[Prometheus exporter](../devices/prometheus.md) page; the ones used below are:

| Option (API key in `tags`) | Use |
|---|---|
| Exporter (`preset`) | Fills in the usual port: `node_exporter`, `windows_exporter`, `cadvisor`, `blackbox_exporter`, `snmp_exporter`, `smartctl_exporter`, `mysqld_exporter`, `postgres_exporter`, `redis_exporter`, `nginx_exporter`, `prometheus`, or `custom` |
| Port (`port`), Protocol (`scheme`) | Where the exporter listens |
| Path (`path`) | Page and query to read, default `/metrics` |
| Keep only (`keep`), Ignore (`drop`) | Regular expressions on the metric name |
| Series limit (`max_series`) | Default 5000 per read |

One monitor reads one page. For an exporter that probes other things through
parameters (blackbox, snmp), add one monitor per target. Counters are stored as
counters: use `rate()` in rules. Metric names below are those the exporters
document; check your version's `/metrics` page for the exact names.

!!! tip "Same recipe through the API"
    Each recipe can be created with `POST /api/targets`, for example
    `{"name":"nas-1","kind":"prometheus","address":"nas-1.lan","interval_secs":60,"tags":{"preset":"node_exporter"}}`.
    See [Terraform and Ansible](automation.md).

The exporter must be reachable from the DumbMonit server, or from a relay agent
chosen on the monitor for a remote site. Bind exporters to a trusted interface
or put them behind basic auth or a bearer token (the monitor supports both).

## node_exporter

Linux host metrics: CPU, memory, filesystems, disk and network counters, load,
pressure. (DumbMonit's own [agent](../devices/agent.md) covers the basics; use
node_exporter when you want its extra collectors.)

```bash
docker run -d --name node_exporter --net host --pid host \
  -v /:/host:ro,rslave quay.io/prometheus/node-exporter:latest \
  --path.rootfs=/host
```

Monitor: exporter `node_exporter` (port 9100), address the host name.

Alert ideas (threshold rules):

- CPU above 90 % for 10 min: `100 - avg by (target) (rate(dumbmonit_node_cpu_seconds_total{mode="idle"}[5m])) * 100 > 90`
- Root filesystem above 90 %: `100 - dumbmonit_node_filesystem_avail_bytes{mountpoint="/"} / dumbmonit_node_filesystem_size_bytes{mountpoint="/"} * 100 > 90`
- Load per core: `dumbmonit_node_load5 > 8` (adjust to the core count)

## windows_exporter

Windows hosts: CPU, memory, logical disks, network, services, IIS, and more.

Install the MSI from the project's releases, then check
`http://host:9182/metrics`. Enable extra collectors with the MSI's
`ENABLED_COLLECTORS` property (for example `cpu,memory,logical_disk,net,os,service`).

Monitor: exporter `windows_exporter` (port 9182).

Alert ideas:

- Free space on `C:` under 10 %: `dumbmonit_windows_logical_disk_free_bytes{volume="C:"} / dumbmonit_windows_logical_disk_size_bytes{volume="C:"} * 100 < 10`
- A service stopped: `dumbmonit_windows_service_state{name="MSSQLSERVER",state="running"} < 1`

## blackbox_exporter

Probes from the exporter's own vantage point: HTTP, TCP, ICMP, DNS, with TLS
details. Useful to probe from a network DumbMonit cannot reach directly.

```yaml
# docker-compose.yml
services:
  blackbox:
    image: quay.io/prometheus/blackbox-exporter:latest
    ports: ["9115:9115"]
```

Monitor: kind Prometheus exporter, address
`http://blackbox.lan:9115/probe?target=https://example.org&module=http_2xx`
(the full URL is accepted as the address). One monitor per target.

Alert ideas: `dumbmonit_probe_success == 0` for 3 min; certificate expiry
`dumbmonit_probe_ssl_earliest_cert_expiry - time() < 14 * 86400`.

## snmp_exporter

Reads SNMP devices through the exporter's modules, for equipment that has no
[DumbMonit SNMP profile](../devices/snmp.md) yet.

```bash
docker run -d --name snmp_exporter -p 9116:9116 prom/snmp_exporter:latest
```

Monitor: address
`http://snmp.lan:9116/snmp?target=192.168.1.10&module=if_mib&auth=public_v2`,
one monitor per device. Add **Keep only** `^(if|sys)` to bound the series.

Alert ideas: `dumbmonit_ifOperStatus{ifDescr="uplink"} != 1`; interface errors
`rate(dumbmonit_ifInErrors[5m]) > 1`.

## mysqld_exporter

MySQL and MariaDB: connections, queries, InnoDB, replication.

```bash
docker run -d --name mysqld_exporter -p 9104:9104 \
  -e MYSQLD_EXPORTER_PASSWORD=change-me \
  quay.io/prometheus/mysqld-exporter:latest \
  --mysqld.address=db.lan:3306 --mysqld.username=exporter
```

Create the database user first, with `PROCESS, REPLICATION CLIENT, SELECT` on the
needed schemas. Monitor: exporter `mysqld_exporter` (port 9104).

Alert ideas: `dumbmonit_mysql_up == 0`; connections near the limit
`dumbmonit_mysql_global_status_threads_connected / dumbmonit_mysql_global_variables_max_connections > 0.85`;
replication lag `dumbmonit_mysql_slave_status_seconds_behind_master > 60`.

## postgres_exporter

PostgreSQL: connections, transactions, locks, replication, database size.

```bash
docker run -d --name postgres_exporter -p 9187:9187 \
  -e DATA_SOURCE_NAME="postgresql://exporter:change-me@db.lan:5432/postgres?sslmode=disable" \
  quay.io/prometheuscommunity/postgres-exporter:latest
```

Monitor: exporter `postgres_exporter` (port 9187).

Alert ideas: `dumbmonit_pg_up == 0`; connections
`sum by (target) (dumbmonit_pg_stat_activity_count) / dumbmonit_pg_settings_max_connections > 0.85`.

## smartctl_exporter

Disk health through smartctl: SMART status, temperature, reallocated sectors.

```bash
docker run -d --name smartctl_exporter --privileged -p 9633:9633 \
  quay.io/prometheuscommunity/smartctl-exporter:latest
```

Monitor: exporter `smartctl_exporter` (port 9633).

Alert ideas: `dumbmonit_smartctl_device_smart_status == 0`;
temperature `dumbmonit_smartctl_device_temperature > 55`.

## ipmi_exporter

IPMI sensors (temperatures, fans, power) of servers that do not offer Redfish.

```bash
docker run -d --name ipmi_exporter -p 9290:9290 \
  quay.io/prometheuscommunity/ipmi-exporter:latest
```

Monitor: exporter `custom`, port 9290, path
`/ipmi?target=bmc-1.lan&module=default`; one monitor per BMC (the exporter needs
the BMC credentials in its own configuration).

Alert ideas: `dumbmonit_ipmi_up == 0`; a sensor in a bad state
`dumbmonit_ipmi_sensor_state > 0` (1 is warning, 2 is critical).

## nginx and Apache exporters

Request and connection counters from the web server's status page. Both are
separate exporters in front of that page: enable `stub_status` on nginx or
`mod_status` on Apache first.

```bash
docker run -d --name nginx_exporter -p 9113:9113 \
  nginx/nginx-prometheus-exporter:latest \
  --nginx.scrape-uri=http://web.lan/stub_status

docker run -d --name apache_exporter -p 9117:9117 \
  lusotycoon/apache-exporter:latest \
  --scrape_uri=http://web.lan/server-status?auto
```

Monitor: nginx exporter `nginx_exporter` (port 9113); Apache exporter `custom`, port 9117.

Alert ideas: `dumbmonit_nginx_up == 0` (`dumbmonit_apache_up == 0`); a drop in
traffic `rate(dumbmonit_nginx_http_requests_total[5m]) < 0.1` over 30 min.

## cAdvisor

Per-container CPU, memory, network and filesystem on a Docker host.

```yaml
services:
  cadvisor:
    image: gcr.io/cadvisor/cadvisor:latest
    ports: ["8080:8080"]
    volumes:
      - /:/rootfs:ro
      - /var/run:/var/run:ro
      - /sys:/sys:ro
      - /var/lib/docker/:/var/lib/docker:ro
```

Monitor: exporter `cadvisor` (port 8080). cAdvisor publishes many series per
container, so set **Keep only** to `^container_(cpu|memory|network)` and raise
the series limit if needed. (DumbMonit's agent already reports Docker container
state; use cAdvisor for resource use per container.)

Alert ideas: container memory near its limit
`dumbmonit_container_memory_working_set_bytes / dumbmonit_container_spec_memory_limit_bytes > 0.9`
(only containers with a limit are meaningful).
