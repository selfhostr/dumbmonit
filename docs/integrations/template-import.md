# Import templates

DumbMonit can turn a monitoring template you already have into an
[integration pack](../packs/index.md): a Zabbix template export, or a
`generator.yml` / `snmp.yml` file from the Prometheus snmp_exporter. The file
is converted on your server, you review the result, then you install it like
any other pack.

DumbMonit does not ship any converted template. You bring the file, so make
sure you have the right to use it (check the licence of the template and of
the repository you took it from). The converted pack stays on your instance;
it is saved in backups with the other packs and never published anywhere.

## Where

**Settings → Monitoring → Integration packs → Import a template**
(administrators only).

1. Choose the file (YAML or XML, at most 2 MiB).
2. Optionally set the pack id, its name, and the `sysObjectID` prefixes its
   SNMP profiles apply to.
3. **Preview conversion** shows what will be created and a report that lists
   every element of the file as *converted*, *partly converted* or *ignored*,
   with the reason. Nothing is stored yet.
4. **Install pack** converts the same file again on the server and installs
   the result. A pack with the same id is updated.

A pack that brings SNMP profiles needs a restart of DumbMonit before the
profiles are used, as for any pack.

Without a `sysObjectID` prefix, an imported SNMP profile is never picked
automatically: choose it in the SNMP profile setting of the device. With a
prefix, it is matched like the built-in profiles, the longest prefix winning.

## Supported formats

| File | What is converted |
|---|---|
| snmp_exporter `snmp.yml` (generated) | Every module becomes an SNMP profile. Numeric metrics (`gauge`, `counter`, `Float`, `Double`, `EnumAsInfo`, `EnumAsStateSet`) become metrics; lookups on the same index become labels; enum values are written in the metric label. **Preferred**: its OIDs are already numeric. |
| snmp_exporter `generator.yml` | Every module becomes an SNMP profile, but only for walk and get entries written as numeric OIDs. Object names (`ifHCInOctets`) are resolved by snmp_exporter's generator from MIB files, which DumbMonit does not read: they are reported as *needs numeric OID*. Overrides set the kind (`counter`, `gauge`) or drop text values (`DisplayString`…). |
| Zabbix template export, YAML (6.0, 7.0) or XML | SNMP items, SNMP discovery rules, HTTP agent items, value maps, macros and simple triggers (below). |

### Zabbix: what is converted

| Zabbix element | DumbMonit |
|---|---|
| `SNMP_AGENT` item with a numeric scalar OID (`….0`, or `get[…]`) | SNMP metric |
| `SNMP_AGENT` `walk[…]` item and its `DEPENDENT` items (`SNMP_WALK_VALUE`) | SNMP metrics |
| SNMP discovery rule (`discovery[{#MACRO},oid,…]`, or `walk[…]` with `SNMP_WALK_TO_JSON`) and its item prototypes (`<column>.{#SNMPINDEX}`) | Walked SNMP metric, one label per discovery macro |
| Discovery filter `NOT_MATCHES_REGEX` | Rows dropped by label (`drop_when_label_matches`) |
| `HTTP_AGENT` item | A source of the pack (path, query fields, headers, `POST` body, basic or bearer authentication) |
| `DEPENDENT` item on an HTTP item, with `JSONPATH` or `REGEX` (output `\1`) | Pack metric |
| Preprocessing `MULTIPLIER` | `scale` |
| Preprocessing `CHANGE_PER_SECOND`, `SIMPLE_CHANGE` | Counter (the rate is charted) |
| Preprocessing `DISCARD_UNCHANGED*`, `CHECK_NOT_SUPPORTED` | Dropped (noted) |
| Value map | Written in the metric label: `Status (1 = up, 2 = down)` |
| User macro `{$X}` | Its default value; in an HTTP pack, an option of the monitor |
| Secret macro (`SECRET_TEXT`, or a name with `PASSWORD`, `TOKEN`, `SECRET`, `API_KEY`) | Never imported: it becomes the monitor's credential |
| Trigger `last()`, `avg()`, `min()` or `max()` compared with `>`, `>=`, `<`, `<=` and a number or a macro | Alert rule (`avg(…,5m)` becomes `avg_over_time(…[300s])`; on a counter, the rate is compared) |

### Zabbix: what is not converted

Each of these is listed in the report with its reason:

- Zabbix agent items (`ZABBIX_PASSIVE`, `ZABBIX_ACTIVE`): the DumbMonit agent
  collects the usual system metrics itself.
- `CALCULATED`, `SCRIPT`, `EXTERNAL`, `SSH`, `TELNET`, `JMX`, `IPMI`,
  database, `SIMPLE`, `INTERNAL` and trap items.
- SNMP OIDs written as MIB names (`IF-MIB::ifHCInOctets`), and OIDs of one
  table row (not a scalar `.0`, not a discovery column).
- Text values (`CHAR`, `TEXT`, `LOG`): DumbMonit records numbers.
- `JAVASCRIPT` and any other preprocessing step, JSONPath functions other than
  `.first()`.
- HTTP discovery rules, LLD overrides, graph and host prototypes.
- Triggers that combine conditions, use other functions (`nodata`, `change`,
  `count`…), compare with `=` or `<>`, or with an expression. Recovery
  expressions and trigger dependencies are dropped: an alert clears with its
  condition, and DumbMonit suppresses alerts through parent devices.
- Graphs, dashboards, web scenarios and linked templates (export and import a
  linked template separately).

## Example

A minimal Zabbix export:

```yaml
zabbix_export:
  version: '7.0'
  templates:
    - template: 'Lab PDU'
      name: 'Lab PDU'
      items:
        - name: 'Total load'
          type: SNMP_AGENT
          snmp_oid: '1.3.6.1.4.1.99999.1.1.0'
          key: pdu.load
          units: A
          preprocessing:
            - type: MULTIPLIER
              parameters: ['0.1']
          triggers:
            - expression: 'avg(/Lab PDU/pdu.load,5m)>{$PDU.LOAD.MAX}'
              name: 'PDU load is high'
              priority: HIGH
      macros:
        - macro: '{$PDU.LOAD.MAX}'
          value: '16'
```

becomes the pack `lab-pdu`:

```yaml
# Converted by DumbMonit from an imported template.
schema: 1
id: "lab-pdu"
version: "1.0.0"
label: "Lab PDU"
summary: "Imported from the Zabbix template \"Lab PDU\"."
rules:
  - expr: "avg_over_time(dumbmonit_lab_pdu_pdu_load[300s])"
    name: "pdu_load_is_high"
    op: ">"
    severity: "critical"
    threshold: 16.0
    title: "PDU load is high"
    unit: "A"
snmp_profiles:
  - id: "lab-pdu"
    include:
      - "system"
    metrics:
      - kind: "gauge"
        label: "Total load"
        name: "lab_pdu_pdu_load"
        oid: "1.3.6.1.4.1.99999.1.1.0"
        scale: 0.1
        unit: "A"
        view: "stat"
    name: "Lab PDU"
```

The report says the trigger was *partly converted*: the macro was replaced by
its default value, 16.

## Limits

| Limit | Value |
|---|---|
| File size | 2 MiB |
| Converted pack | 256 KiB, like any pack |
| HTTP sources | 16 per pack |
| Metrics | 256 per SNMP profile, 256 HTTP metrics per pack |
| Alert rules | 64 per pack |
| Report | the first 2,000 elements are listed |

The import is also available from the API:
`POST /api/import/templates/preview` and `POST /api/import/templates/apply`,
with `{"content": "<the file>", "id"?: "…", "label"?: "…", "sysobjectid"?: ["…"]}`
(see the [API reference](../reference/api.md)).
