# DumbMonit compared

DumbMonit is a monitoring tool for homelabs and small teams: one container, a
web interface to configure everything, built-in alert rules, and about a hundred
integrations. It does not try to be everything. This page says what it does, what
it deliberately leaves out, and when another tool is the better choice.

The other tools are described from their public documentation and may have changed
since; check their own pages before deciding.

## What DumbMonit does not do

- **No APM or distributed tracing.** It can receive OpenTelemetry metrics (see
  [OTLP](devices/otlp.md)), not traces or application profiling.
- **No free-form dashboard builder.** You get the overview, the metrics explorer
  and per-monitor graphs, not a canvas of panels you design. If your team lives in
  custom dashboards, use Grafana (it can read DumbMonit's VictoriaMetrics).
- **No NetFlow, sFlow or IPFIX** traffic analysis.
- **No active/active cluster.** One server runs the checks and the alerting. The
  time series can sit in an external VictoriaMetrics, see
  [High availability](install/high-availability.md) for what is and is not covered.
- **No browser-driven synthetic scenarios.** HTTP checks, keyword checks and
  multi-step [HTTP scenarios](devices/http-scenario.md) exist; a real browser does
  not run.
- **No IPMI.** Server hardware is read through Redfish, or a Prometheus
  `ipmi_exporter` (see [Prometheus exporters](integrations/prometheus-exporters.md)).

## Side by side

| | What it is best at | Where DumbMonit differs |
|---|---|---|
| **Zabbix** | Large, template-driven monitoring with a huge community template library, custom items, triggers and proxies. | DumbMonit installs in one container and needs far less configuration, but has no equivalent of Zabbix's template ecosystem or user-written items and low-level discovery (see [Integration packs](packs/index.md) for what it offers). |
| **Checkmk** | Agent-based infrastructure monitoring with service discovery and a large plugin catalogue, in free and commercial editions. | Comparable ground (agents, SNMP, alerting, notification rules). DumbMonit is a single product with no paid edition; Checkmk has a much larger plugin catalogue. |
| **Netdata** | Per-second metrics and instant dashboards on each machine, with anomaly detection. | DumbMonit checks at an interval of its choice (60 s by default), stores history centrally and adds device types Netdata does not focus on (network gear, hypervisors, NAS, status pages). |
| **Prometheus + Grafana** | Maximum flexibility: PromQL, any exporter, any dashboard, Alertmanager routing. | You assemble and maintain several components there. DumbMonit is one container with the UI, rules and notification channels built in, and it can [scrape Prometheus exporters](devices/prometheus.md) itself. Pick Prometheus if you want to write queries and dashboards for everything. |
| **Uptime Kuma** | Simple uptime checks and status pages, very easy to start. | DumbMonit covers the same checks and [status pages](using/status-pages.md) and adds agents, SNMP, hypervisors, logs, topology and alert logic (dependencies, maintenance, forecasts). If you only need uptime checks, Uptime Kuma is lighter. |
| **Beszel** | A lightweight hub and agents for server and Docker stats. | DumbMonit's agent covers servers and containers too, plus network devices and integrations. Beszel is smaller and has a narrower scope. |
| **LibreNMS** | Network monitoring with automatic discovery and a very large SNMP device library. | DumbMonit ships a [large set of SNMP profiles](devices/snmp.md) with automatic detection, but LibreNMS is the more established tool for network-only estates. |
| **PRTG** | A commercial all-in-one Windows-based monitoring product with sensors and maps. | DumbMonit is free and open source (MIT), runs in a Linux container, and has no sensor-based licensing. PRTG offers a vendor-supported, sensor-licensed model. |

## When to pick something else

- You need **traces, profiling or application performance** monitoring: use an
  APM stack.
- You want to **design your own dashboards** over any metric: Grafana, with
  DumbMonit or Prometheus as a source.
- You monitor **thousands of devices** with custom templates and a dedicated
  team: Zabbix or Checkmk are built for that.
- You need **flow analysis** (who talks to whom, how much): a NetFlow collector.
- You need **active/active high availability** for the monitoring itself.

If none of that applies, and you want something you can run in a minute and keep
open all day, [start here](install/quickstart.md).
