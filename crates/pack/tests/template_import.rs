//! Import de modèles fournis par l'utilisateur : snmp_exporter et Zabbix.
//!
//! Les fichiers ci-dessous sont écrits pour ces tests (équipements fictifs,
//! numéro d'entreprise 99999) : aucun modèle tiers n'est repris.

use dumbmonit_pack::Pack;
use dumbmonit_pack::import::{Conversion, ImportOptions, Outcome, SourceFormat, convert};

fn run(text: &str) -> Conversion {
    convert(text, &ImportOptions::default()).expect("fichier reconnu")
}

fn installable(conversion: &Conversion) -> Pack {
    assert!(conversion.is_installable(), "{:?}\n{}", conversion.errors, conversion.yaml);
    Pack::parse(&conversion.yaml).expect("paquet valide")
}

fn outcome(conversion: &Conversion, name: &str) -> Outcome {
    conversion
        .report
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("{name} absent du rapport : {:#?}", conversion.report))
        .outcome
}

fn reason<'a>(conversion: &'a Conversion, name: &str) -> &'a str {
    &conversion.report.iter().find(|entry| entry.name == name).expect("entrée").reason
}

const SNMP_YML: &str = r#"
auths:
  public_v2:
    community: public
    version: 2
modules:
  acme_pdu:
    walk:
      - 1.3.6.1.4.1.99999.1
    metrics:
      - name: acmePduUptime
        oid: 1.3.6.1.4.1.99999.1.1
        type: gauge
        help: Seconds since the last restart
      - name: acmeOutletLoad
        oid: 1.3.6.1.4.1.99999.1.2.1.3
        type: gauge
        help: Load of an outlet, in tenths of an ampere
        indexes:
          - labelname: acmeOutletIndex
            type: gauge
        lookups:
          - labels: [acmeOutletIndex]
            labelname: acmeOutletName
            oid: 1.3.6.1.4.1.99999.1.2.1.2
            type: DisplayString
        scale: 0.1
      - name: acmeOutletState
        oid: 1.3.6.1.4.1.99999.1.2.1.4
        type: EnumAsInfo
        indexes: [{labelname: acmeOutletIndex, type: gauge}]
        enum_values: {1: on, 2: off}
      - name: acmeOutletEnergy
        oid: 1.3.6.1.4.1.99999.1.2.1.5
        type: counter
        indexes: [{labelname: acmeOutletIndex, type: gauge}]
      - name: acmeOutletName
        oid: 1.3.6.1.4.1.99999.1.2.1.2
        type: DisplayString
        indexes: [{labelname: acmeOutletIndex, type: gauge}]
"#;

#[test]
fn un_snmp_yml_devient_un_profil() {
    let conversion = run(SNMP_YML);
    assert_eq!(conversion.format, SourceFormat::SnmpExporterSnmp);
    assert_eq!(conversion.pack_id, "acme-pdu");
    let pack = installable(&conversion);
    assert!(!pack.has_collector());
    assert_eq!(pack.snmp_profiles().len(), 1);
    assert_eq!(conversion.summary.snmp_metrics, 4);
    let names: Vec<&str> = pack.metric_names().collect();
    assert!(names.contains(&"dumbmonit_acme_pdu_acme_outlet_load"), "{names:?}");
    assert!(names.contains(&"dumbmonit_acme_pdu_acme_pdu_uptime"), "{names:?}");

    // Le scalaire reçoit son « .0 », la colonne garde son lookup en étiquette.
    assert!(conversion.yaml.contains("\"1.3.6.1.4.1.99999.1.1.0\""), "{}", conversion.yaml);
    assert!(conversion.yaml.contains("acme_outlet_name: \"1.3.6.1.4.1.99999.1.2.1.2\""));
    assert!(conversion.yaml.contains("acmeOutletState (1 = on, 2 = off)"));
    assert_eq!(outcome(&conversion, "acme_pdu.acmeOutletName"), Outcome::Ignored);
    assert_eq!(outcome(&conversion, "acme_pdu.acmeOutletEnergy"), Outcome::Converted);
    assert_eq!(outcome(&conversion, "acme_pdu.acmeOutletState"), Outcome::Partial);
    assert_eq!(outcome(&conversion, "auths"), Outcome::Ignored);
}

#[test]
fn le_sysobjectid_choisi_rejoint_le_profil() {
    let options = ImportOptions {
        id: Some("my-pdu".into()),
        label: Some("My PDU".into()),
        sysobjectid: vec!["1.3.6.1.4.1.99999".into()],
    };
    let conversion = convert(SNMP_YML, &options).unwrap();
    assert_eq!(conversion.pack_id, "my-pdu");
    assert_eq!(conversion.label, "My PDU");
    assert!(conversion.yaml.contains("sysobjectid:\n"), "{}", conversion.yaml);
    installable(&conversion);
    let refused = ImportOptions { sysobjectid: vec!["acme".into()], ..ImportOptions::default() };
    assert!(convert(SNMP_YML, &refused).is_err());
}

const GENERATOR_YML: &str = r#"
modules:
  acme_small:
    walk:
      - sysUpTime
      - 1.3.6.1.4.1.99999.2.1.0
      - 1.3.6.1.4.1.99999.2.3.1.2
    lookups:
      - source_indexes: [acmeIfIndex]
        lookup: 1.3.6.1.4.1.99999.2.3.1.1
      - source_indexes: [acmeIfIndex]
        lookup: acmeIfAlias
    overrides:
      1.3.6.1.4.1.99999.2.3.1.2:
        type: counter
    max_repetitions: 25
"#;

#[test]
fn un_generator_yml_ne_garde_que_les_oid_numeriques() {
    let conversion = run(GENERATOR_YML);
    assert_eq!(conversion.format, SourceFormat::SnmpExporterGenerator);
    let pack = installable(&conversion);
    assert_eq!(conversion.summary.snmp_metrics, 2);
    assert_eq!(outcome(&conversion, "acme_small.sysUpTime"), Outcome::Ignored);
    assert!(reason(&conversion, "acme_small.sysUpTime").contains("needs numeric OID"));
    assert_eq!(outcome(&conversion, "acmeIfAlias"), Outcome::Ignored);
    assert_eq!(outcome(&conversion, "1.3.6.1.4.1.99999.2.3.1.1"), Outcome::Partial);
    assert!(conversion.yaml.contains("kind: \"counter\""), "{}", conversion.yaml);
    assert!(conversion.yaml.contains("lookup_1: \"1.3.6.1.4.1.99999.2.3.1.1\""));
    assert_eq!(pack.snmp_profiles()[0].id, "acme-small");
}

const ZABBIX_SNMP: &str = r#"
zabbix_export:
  version: '6.0'
  templates:
    - uuid: 3c1f0a7b9e2d4c5f8a6b1d2e3f4a5b6c
      template: 'Acme UPS SNMP'
      name: 'Acme UPS SNMP'
      description: 'A small template written for the DumbMonit tests.'
      groups:
        - name: Templates
      items:
        - name: 'Battery charge'
          type: SNMP_AGENT
          snmp_oid: '1.3.6.1.4.1.99999.3.1.0'
          key: acme.battery.charge
          units: '%'
          triggers:
            - expression: 'last(/Acme UPS SNMP/acme.battery.charge)<{$CHARGE.MIN}'
              name: 'Battery charge is low'
              priority: HIGH
        - name: 'Output status'
          type: SNMP_AGENT
          snmp_oid: '1.3.6.1.4.1.99999.3.2.0'
          key: acme.output.status
          valuemap:
            name: 'Acme output'
        - name: 'Uptime'
          type: SNMP_AGENT
          snmp_oid: 'SNMPv2-MIB::sysUpTime.0'
          key: acme.uptime
        - name: 'Agent ping'
          key: agent.ping
        - name: 'Input voltage'
          type: SNMP_AGENT
          snmp_oid: '1.3.6.1.4.1.99999.3.3.0'
          key: acme.input.voltage
          units: V
          preprocessing:
            - type: MULTIPLIER
              parameters:
                - '0.1'
          triggers:
            - expression: 'avg(/Acme UPS SNMP/acme.input.voltage,5m)>250'
              name: 'Input voltage is high'
              priority: WARNING
            - expression: 'nodata(/Acme UPS SNMP/acme.input.voltage,10m)=1'
              name: 'No voltage data'
      discovery_rules:
        - name: 'Outlet discovery'
          type: SNMP_AGENT
          snmp_oid: 'discovery[{#OUTLET.NAME},1.3.6.1.4.1.99999.3.4.1.2]'
          key: acme.outlet.discovery
          filter:
            conditions:
              - macro: '{#OUTLET.NAME}'
                value: '{$OUTLET.SKIP}'
                operator: NOT_MATCHES_REGEX
                formulaid: A
          item_prototypes:
            - name: 'Outlet {#OUTLET.NAME}: energy'
              type: SNMP_AGENT
              snmp_oid: '1.3.6.1.4.1.99999.3.4.1.5.{#SNMPINDEX}'
              key: 'acme.outlet.energy[{#SNMPINDEX}]'
              units: Wh
              preprocessing:
                - type: CHANGE_PER_SECOND
                  parameters:
                    - ''
              trigger_prototypes:
                - expression: 'last(/Acme UPS SNMP/acme.outlet.energy[{#SNMPINDEX}])>1000'
                  name: 'Outlet {#OUTLET.NAME} draws a lot'
                  priority: AVERAGE
            - name: 'Outlet {#OUTLET.NAME}: label'
              type: SNMP_AGENT
              snmp_oid: '1.3.6.1.4.1.99999.3.4.1.2.{#SNMPINDEX}'
              key: 'acme.outlet.label[{#SNMPINDEX}]'
              value_type: CHAR
      macros:
        - macro: '{$CHARGE.MIN}'
          value: '30'
        - macro: '{$OUTLET.SKIP}'
          value: '^spare'
      valuemaps:
        - name: 'Acme output'
          mappings:
            - value: '1'
              newvalue: online
            - value: '2'
              newvalue: 'on battery'
      dashboards:
        - name: Overview
"#;

#[test]
fn un_template_zabbix_snmp_donne_profil_et_regles() {
    let conversion = run(ZABBIX_SNMP);
    assert_eq!(conversion.format, SourceFormat::ZabbixYaml);
    assert_eq!(conversion.pack_id, "acme-ups-snmp");
    let pack = installable(&conversion);
    assert_eq!(conversion.summary.snmp_metrics, 4, "{}", conversion.yaml);

    assert_eq!(outcome(&conversion, "acme.battery.charge"), Outcome::Converted);
    assert_eq!(outcome(&conversion, "acme.output.status"), Outcome::Partial);
    assert!(conversion.yaml.contains("Output status (1 = online, 2 = on battery)"));
    assert!(reason(&conversion, "acme.uptime").contains("needs numeric OID"));
    assert!(reason(&conversion, "agent.ping").contains("Zabbix agent"));
    assert_eq!(outcome(&conversion, "acme.outlet.label[{#SNMPINDEX}]"), Outcome::Ignored);
    assert_eq!(outcome(&conversion, "Outlet discovery"), Outcome::Converted);
    assert_eq!(outcome(&conversion, "No voltage data"), Outcome::Ignored);
    assert_eq!(outcome(&conversion, "1 dashboards"), Outcome::Ignored);

    // La découverte : colonne parcourue, étiquette et filtre tirés de la macro.
    let yaml = &conversion.yaml;
    assert!(yaml.contains("outlet_name: \"1.3.6.1.4.1.99999.3.4.1.2\""), "{yaml}");
    assert!(yaml.contains("drop_when_label_matches:"), "{yaml}");
    assert!(yaml.contains("\"^spare\""), "{yaml}");
    assert!(yaml.contains("scale: 0.1"), "{yaml}");

    let rules = pack.rules();
    assert_eq!(rules.len(), 3, "{rules:#?}");
    let low = rules.iter().find(|r| r.title == "Battery charge is low").unwrap();
    assert_eq!(low.expr, "dumbmonit_acme_ups_snmp_acme_battery_charge");
    assert_eq!((low.op.as_str(), low.threshold), ("<", 30.0));
    assert_eq!(low.severity, "critical");
    assert_eq!(low.unit, "%");
    let high = rules.iter().find(|r| r.title == "Input voltage is high").unwrap();
    assert_eq!(high.expr, "avg_over_time(dumbmonit_acme_ups_snmp_acme_input_voltage[300s])");
    assert_eq!(high.severity, "warning");
    let outlet = rules.iter().find(|r| r.title.contains("draws a lot")).unwrap();
    assert!(outlet.expr.starts_with("rate(dumbmonit_acme_ups_snmp_acme_outlet_energy"));
}

const ZABBIX_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<zabbix_export>
  <version>6.0</version>
  <templates>
    <template>
      <template>Acme Switch</template>
      <name>Acme Switch</name>
      <items>
        <item>
          <name>CPU load</name>
          <type>SNMP_AGENT</type>
          <snmp_oid>1.3.6.1.4.1.99999.4.1.0</snmp_oid>
          <key>acme.cpu</key>
          <units>%</units>
          <triggers>
            <trigger>
              <expression>min(/Acme Switch/acme.cpu,10m)&gt;=90</expression>
              <name>CPU busy</name>
              <priority>DISASTER</priority>
            </trigger>
          </triggers>
        </item>
      </items>
    </template>
  </templates>
</zabbix_export>
"#;

#[test]
fn un_export_xml_se_lit_comme_le_yaml() {
    let conversion = run(ZABBIX_XML);
    assert_eq!(conversion.format, SourceFormat::ZabbixXml);
    let pack = installable(&conversion);
    assert_eq!(conversion.summary.snmp_metrics, 1);
    let rule = &pack.rules()[0];
    assert_eq!(rule.expr, "min_over_time(dumbmonit_acme_switch_acme_cpu[600s])");
    assert_eq!((rule.op.as_str(), rule.threshold), (">=", 90.0));
    assert_eq!(rule.severity, "critical");
}

const ZABBIX_HTTP: &str = r#"
zabbix_export:
  version: '7.0'
  templates:
    - template: 'Acme API by HTTP'
      name: 'Acme API by HTTP'
      items:
        - name: 'Get status'
          type: HTTP_AGENT
          key: acme.api.get
          value_type: TEXT
          url: 'https://{HOST.CONN}:{$ACME.PORT}/api/v2/status'
          query_fields:
            - name: detail
              value: '{$ACME.DETAIL}'
          headers:
            - name: X-Api-Key
              value: '{$ACME.API.TOKEN}'
        - name: 'Active sessions'
          type: DEPENDENT
          key: acme.api.sessions
          master_item:
            key: acme.api.get
          preprocessing:
            - type: JSONPATH
              parameters:
                - '$.sessions.active'
          triggers:
            - expression: 'last(/Acme API by HTTP/acme.api.sessions)>{$ACME.SESSIONS.MAX}'
              name: 'Too many sessions'
              priority: WARNING
        - name: 'Requests'
          type: DEPENDENT
          key: acme.api.requests
          master_item:
            key: acme.api.get
          preprocessing:
            - type: JSONPATH
              parameters:
                - '$.requests.total'
            - type: CHANGE_PER_SECOND
              parameters:
                - ''
        - name: 'Computed'
          type: DEPENDENT
          key: acme.api.js
          master_item:
            key: acme.api.get
          preprocessing:
            - type: JAVASCRIPT
              parameters:
                - 'return 1'
        - name: 'Ratio'
          type: CALCULATED
          key: acme.ratio
          params: 'last(//acme.api.sessions)/2'
      macros:
        - macro: '{$ACME.PORT}'
          value: '8443'
        - macro: '{$ACME.DETAIL}'
          value: full
        - macro: '{$ACME.API.TOKEN}'
          type: SECRET_TEXT
        - macro: '{$ACME.SESSIONS.MAX}'
          value: '200'
"#;

#[test]
fn un_template_zabbix_http_devient_un_paquet_qui_interroge() {
    let conversion = run(ZABBIX_HTTP);
    let pack = installable(&conversion);
    assert!(pack.has_collector());
    assert_eq!(pack.kind(), "pack.acme-api-by-http");
    assert_eq!(conversion.summary.http_sources, 1);
    assert_eq!(conversion.summary.http_metrics, 2);
    assert_eq!(conversion.summary.options, 2);

    let yaml = &conversion.yaml;
    assert!(yaml.contains("scheme: \"https\""), "{yaml}");
    assert!(yaml.contains("default_port: 8443"), "{yaml}");
    assert!(yaml.contains("\"api_token\""), "{yaml}");
    assert!(yaml.contains("/api/v2/status?detail={{option.acme_detail}}"), "{yaml}");
    assert!(yaml.contains("X-Api-Key: \"{{credential.token}}\""), "{yaml}");
    assert!(!yaml.contains("SECRET"), "aucune valeur secrète n'est recopiée");

    assert!(reason(&conversion, "acme.api.js").contains("JAVASCRIPT"));
    assert!(reason(&conversion, "acme.ratio").contains("calculated"));
    assert_eq!(outcome(&conversion, "secret macros"), Outcome::Partial);

    let rule = &pack.rules()[0];
    assert_eq!(rule.expr, "dumbmonit_acme_api_by_http_acme_api_sessions");
    assert_eq!(rule.threshold, 200.0);
    assert_eq!(rule.threshold_option.as_deref(), Some("acme_sessions_max"));
}

#[test]
fn un_fichier_inconnu_ou_trop_gros_est_refuse() {
    assert!(convert("hello: world\n", &ImportOptions::default()).is_err());
    assert!(convert("<html></html>", &ImportOptions::default()).is_err());
    let huge = format!("modules: {{}}\n#{}", "x".repeat(dumbmonit_pack::import::MAX_INPUT_BYTES));
    assert!(convert(&huge, &ImportOptions::default()).unwrap_err().0.contains("KiB"));
}

#[test]
fn un_template_sans_rien_de_convertible_le_dit() {
    let only_agent = r#"
zabbix_export:
  version: '6.0'
  templates:
    - template: 'Linux by agent'
      name: 'Linux by agent'
      items:
        - name: 'CPU'
          key: system.cpu.util
"#;
    let conversion = run(only_agent);
    assert!(!conversion.is_installable());
    assert!(conversion.errors[0].contains("nothing"), "{:?}", conversion.errors);
    assert_eq!(conversion.summary.ignored, 1);
}
