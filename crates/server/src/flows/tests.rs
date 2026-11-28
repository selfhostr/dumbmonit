//! Décodeurs sur des datagrammes construits à la main, robustesse face aux
//! entrées mal formées, agrégation et filtrage des exportateurs.

use std::net::IpAddr;

use chrono::{TimeZone, Utc};

use super::aggregate::{Aggregator, MAX_SERIES, TOP_N};
use super::netflow::{self, DecodeError, MAX_TEMPLATES_PER_EXPORTER, TEMPLATE_TTL_SECS, Templates};
use super::{FlowRecord, Hub, Protocol, Settings, sflow};

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn be16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn be32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

// ------------------------------------------------------------------ NetFlow v5

/// Un enregistrement v5 : source, destination, ports, protocole, octets, paquets.
type V5Record = ([u8; 4], [u8; 4], u16, u16, u8, u32, u32);

fn v5(records: &[V5Record], sampling: u16) -> Vec<u8> {
    let mut out = Vec::new();
    be16(&mut out, 5);
    be16(&mut out, records.len() as u16);
    out.extend_from_slice(&[0; 18]);
    be16(&mut out, sampling);
    for (src, dst, sport, dport, proto, packets, bytes) in records {
        out.extend_from_slice(src);
        out.extend_from_slice(dst);
        out.extend_from_slice(&[0; 8]);
        be32(&mut out, *packets);
        be32(&mut out, *bytes);
        out.extend_from_slice(&[0; 8]);
        be16(&mut out, *sport);
        be16(&mut out, *dport);
        out.extend_from_slice(&[0, 0x18, *proto, 0]);
        out.extend_from_slice(&[0; 8]);
    }
    out
}

#[test]
fn a_v5_datagram_gives_its_records_scaled_by_the_sampling_interval() {
    let data = v5(
        &[
            ([10, 0, 0, 2], [1, 1, 1, 1], 51_000, 443, 6, 10, 1500),
            ([10, 0, 0, 3], [8, 8, 8, 8], 40_000, 53, 17, 1, 80),
        ],
        // Mode 01, intervalle 100.
        0x4000 | 100,
    );
    let mut templates = Templates::default();
    let decoded = netflow::decode(&data, ip("10.0.0.1"), 0, &mut templates).unwrap();
    assert_eq!(decoded.version, 5);
    assert_eq!(decoded.records.len(), 2);
    let first = &decoded.records[0];
    assert_eq!((first.src, first.dst), (ip("10.0.0.2"), ip("1.1.1.1")));
    assert_eq!((first.proto, first.port), (6, 443));
    assert_eq!((first.bytes, first.packets), (150_000, 1000));
    assert_eq!(decoded.records[1].port, 53);
}

// ------------------------------------------------------------- v9 et IPFIX

fn set(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    be16(&mut out, id);
    be16(&mut out, (body.len() + 4) as u16);
    out.extend_from_slice(body);
    out
}

fn template(id: u16, fields: &[(u16, u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    be16(&mut out, id);
    be16(&mut out, fields.len() as u16);
    for (field, len) in fields {
        be16(&mut out, *field);
        be16(&mut out, *len);
    }
    out
}

fn v9(source_id: u32, sets: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    be16(&mut out, 9);
    be16(&mut out, sets.len() as u16);
    out.extend_from_slice(&[0; 12]);
    be32(&mut out, source_id);
    sets.iter().for_each(|s| out.extend_from_slice(s));
    out
}

/// src, dst, proto, sport, dport, bytes, packets : 21 octets.
const V9_FIELDS: [(u16, u16); 7] = [(8, 4), (12, 4), (4, 1), (7, 2), (11, 2), (1, 4), (2, 4)];

fn v9_record(src: [u8; 4], dst: [u8; 4], proto: u8, sport: u16, dport: u16, bytes: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&src);
    out.extend_from_slice(&dst);
    out.push(proto);
    be16(&mut out, sport);
    be16(&mut out, dport);
    be32(&mut out, bytes);
    be32(&mut out, 3);
    out
}

#[test]
fn v9_data_is_read_with_the_template_of_its_exporter_and_source_id() {
    let exporter = ip("192.168.1.1");
    let mut templates = Templates::default();
    let mut data = v9_record([192, 168, 1, 20], [9, 9, 9, 9], 6, 443, 60_000, 4000);
    data.push(0); // remplissage
    let data_set = set(256, &data);

    // Les données avant le gabarit : rien à lire, mais comptées.
    let early =
        netflow::decode(&v9(1, std::slice::from_ref(&data_set)), exporter, 0, &mut templates)
            .unwrap();
    assert!(early.records.is_empty());
    assert_eq!(early.missing_template, 1);

    let packet = v9(1, &[set(0, &template(256, &V9_FIELDS)), data_set.clone()]);
    let decoded = netflow::decode(&packet, exporter, 10, &mut templates).unwrap();
    assert_eq!(decoded.records.len(), 1);
    let record = &decoded.records[0];
    assert_eq!((record.src, record.dst, record.port), (ip("192.168.1.20"), ip("9.9.9.9"), 443));
    assert_eq!((record.bytes, record.packets), (4000, 3));

    // Un autre source_id, ou un autre exportateur, n'a pas ce gabarit.
    let other_id =
        netflow::decode(&v9(2, std::slice::from_ref(&data_set)), exporter, 20, &mut templates);
    assert_eq!(other_id.unwrap().missing_template, 1);
    let other_ip = netflow::decode(
        &v9(1, std::slice::from_ref(&data_set)),
        ip("10.0.0.9"),
        20,
        &mut templates,
    );
    assert_eq!(other_ip.unwrap().missing_template, 1);

    // Sans rafraîchissement, le gabarit expire.
    let late = TEMPLATE_TTL_SECS + 20;
    let expired = netflow::decode(&v9(1, &[data_set]), exporter, late, &mut templates).unwrap();
    assert_eq!(expired.missing_template, 1);
}

#[test]
fn the_templates_of_one_exporter_are_capped() {
    let exporter = ip("192.168.1.1");
    let mut templates = Templates::default();
    let body: Vec<u8> = (0..(MAX_TEMPLATES_PER_EXPORTER as u16 + 20))
        .flat_map(|n| template(256 + n, &V9_FIELDS))
        .collect();
    // Plusieurs ensembles pour rester sous 64 Kio par ensemble.
    for chunk in body.chunks(32 * (4 + 4 * V9_FIELDS.len())) {
        netflow::decode(&v9(1, &[set(0, chunk)]), exporter, 0, &mut templates).unwrap();
    }
    assert_eq!(templates.len(), MAX_TEMPLATES_PER_EXPORTER);
    // Un autre exportateur garde sa place.
    let other = v9(1, &[set(0, &template(256, &V9_FIELDS))]);
    netflow::decode(&other, ip("192.168.1.2"), 0, &mut templates).unwrap();
    assert_eq!(templates.len(), MAX_TEMPLATES_PER_EXPORTER + 1);
}

fn ipfix(domain: u32, sets: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = sets.concat();
    let mut out = Vec::new();
    be16(&mut out, 10);
    be16(&mut out, (body.len() + 16) as u16);
    out.extend_from_slice(&[0; 8]);
    be32(&mut out, domain);
    out.extend_from_slice(&body);
    out
}

#[test]
fn ipfix_reads_ipv6_variable_and_enterprise_fields_and_honours_withdrawal() {
    let exporter = ip("10.1.1.1");
    let mut templates = Templates::default();
    let mut tpl = Vec::new();
    be16(&mut tpl, 300);
    be16(&mut tpl, 9);
    for (field, len) in [(27u16, 16u16), (28, 16), (4, 1), (7, 2), (11, 2), (1, 8), (2, 8)] {
        be16(&mut tpl, field);
        be16(&mut tpl, len);
    }
    // Élément d'un constructeur (numéro d'entreprise), puis un champ variable.
    be16(&mut tpl, 0x8000 | 100);
    be16(&mut tpl, 4);
    be32(&mut tpl, 9);
    be16(&mut tpl, 82);
    be16(&mut tpl, 0xFFFF);

    let mut record = Vec::new();
    record.extend_from_slice(&ip_bytes("2001:db8::1"));
    record.extend_from_slice(&ip_bytes("2001:db8::2"));
    record.push(17);
    be16(&mut record, 5353);
    be16(&mut record, 5353);
    record.extend_from_slice(&900u64.to_be_bytes());
    record.extend_from_slice(&2u64.to_be_bytes());
    be32(&mut record, 0xDEAD_BEEF);
    record.push(3);
    record.extend_from_slice(b"eth");

    let packet = ipfix(7, &[set(2, &tpl), set(300, &record)]);
    let decoded = netflow::decode(&packet, exporter, 0, &mut templates).unwrap();
    assert_eq!(decoded.version, 10);
    assert_eq!(decoded.records.len(), 1);
    let flow = &decoded.records[0];
    assert_eq!((flow.src, flow.dst), (ip("2001:db8::1"), ip("2001:db8::2")));
    assert_eq!((flow.proto, flow.port, flow.bytes, flow.packets), (17, 5353, 900, 2));

    // Retrait : nombre de champs nul.
    let mut withdraw = Vec::new();
    be16(&mut withdraw, 300);
    be16(&mut withdraw, 0);
    let packet = ipfix(7, &[set(2, &withdraw), set(300, &record)]);
    let after = netflow::decode(&packet, exporter, 1, &mut templates).unwrap();
    assert!(after.records.is_empty());
    assert_eq!(after.missing_template, 1);
}

fn ip_bytes(text: &str) -> Vec<u8> {
    match ip(text) {
        IpAddr::V4(v4) => v4.octets().to_vec(),
        IpAddr::V6(v6) => v6.octets().to_vec(),
    }
}

// -------------------------------------------------------------------- sFlow

fn record(format: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    be32(&mut out, format);
    be32(&mut out, body.len() as u32);
    out.extend_from_slice(body);
    out
}

fn raw_header(frame: &[u8], frame_length: u32) -> Vec<u8> {
    let mut body = Vec::new();
    be32(&mut body, 1);
    be32(&mut body, frame_length);
    be32(&mut body, 4);
    be32(&mut body, frame.len() as u32);
    body.extend_from_slice(frame);
    while body.len() % 4 != 0 {
        body.push(0);
    }
    record(1, &body)
}

/// Trame Ethernet étiquetée VLAN, IPv4 + TCP 443 → 51000.
fn tagged_tcp_frame() -> Vec<u8> {
    let mut frame = vec![0u8; 12];
    be16(&mut frame, 0x8100);
    be16(&mut frame, 10);
    be16(&mut frame, 0x0800);
    frame.extend_from_slice(&[0x45, 0, 0, 52, 0, 0, 0x40, 0, 64, 6, 0, 0]);
    frame.extend_from_slice(&[192, 168, 1, 50, 151, 101, 1, 69]);
    be16(&mut frame, 443);
    be16(&mut frame, 51_000);
    frame.extend_from_slice(&[0; 16]);
    frame
}

fn udp6_frame() -> Vec<u8> {
    let mut frame = vec![0u8; 12];
    be16(&mut frame, 0x86DD);
    frame.extend_from_slice(&[0x60, 0, 0, 0, 0, 8, 17, 64]);
    frame.extend_from_slice(&ip_bytes("fd00::10"));
    frame.extend_from_slice(&ip_bytes("fd00::20"));
    be16(&mut frame, 40_000);
    be16(&mut frame, 123);
    frame.extend_from_slice(&[0; 4]);
    frame
}

fn flow_sample(expanded: bool, rate: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    be32(&mut body, 1);
    if expanded {
        be32(&mut body, 0);
    }
    be32(&mut body, 3);
    be32(&mut body, rate);
    let words = if expanded { 6 } else { 4 };
    (0..words).for_each(|_| be32(&mut body, 0));
    be32(&mut body, records.len() as u32);
    records.iter().for_each(|r| body.extend_from_slice(r));
    record(if expanded { 3 } else { 1 }, &body)
}

fn sflow_datagram(samples: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    be32(&mut out, 5);
    be32(&mut out, 1);
    out.extend_from_slice(&[10, 0, 0, 254]);
    out.extend_from_slice(&[0; 12]);
    be32(&mut out, samples.len() as u32);
    samples.iter().for_each(|s| out.extend_from_slice(s));
    out
}

fn sflow_sample_packet() -> Vec<u8> {
    let mut sampled_v4 = Vec::new();
    be32(&mut sampled_v4, 1000);
    be32(&mut sampled_v4, 17);
    sampled_v4.extend_from_slice(&[10, 0, 0, 7, 10, 0, 0, 8]);
    be32(&mut sampled_v4, 5000);
    be32(&mut sampled_v4, 514);
    be32(&mut sampled_v4, 0);
    be32(&mut sampled_v4, 0);
    let counters = record(2, &[0; 8]);
    sflow_datagram(&[
        flow_sample(false, 512, &[record(1001, &[0; 4]), raw_header(&tagged_tcp_frame(), 1514)]),
        counters,
        flow_sample(true, 1024, &[raw_header(&udp6_frame(), 90)]),
        flow_sample(false, 0, &[record(3, &sampled_v4)]),
    ])
}

#[test]
fn sflow_samples_are_decoded_and_scaled_by_their_rate() {
    let records = sflow::decode(&sflow_sample_packet()).unwrap();
    assert_eq!(records.len(), 3);
    let tcp = &records[0];
    assert_eq!((tcp.src, tcp.dst), (ip("192.168.1.50"), ip("151.101.1.69")));
    assert_eq!((tcp.proto, tcp.port), (6, 443));
    assert_eq!((tcp.bytes, tcp.packets), (1514 * 512, 512));
    let udp = &records[1];
    assert_eq!((udp.src, udp.dst, udp.port), (ip("fd00::10"), ip("fd00::20"), 123));
    assert_eq!((udp.bytes, udp.packets), (90 * 1024, 1024));
    // Taux nul : traité comme 1.
    let v4 = &records[2];
    assert_eq!((v4.src, v4.port, v4.bytes, v4.packets), (ip("10.0.0.7"), 514, 1000, 1));
    assert_eq!(sflow::decode(&[0, 0, 0, 4]), Err(sflow::DecodeError::Version));
}

// ------------------------------------------------------------- robustesse

/// Générateur pseudo-aléatoire déterministe (LCG) : pas de dépendance.
struct Lcg(u64);

impl Lcg {
    fn byte(&mut self) -> u8 {
        const MUL: u64 = 6_364_136_223_846_793_005;
        const INC: u64 = 1_442_695_040_888_963_407;
        self.0 = self.0.wrapping_mul(MUL).wrapping_add(INC);
        (self.0 >> 33) as u8
    }
}

fn valid_packets() -> Vec<(Protocol, Vec<u8>)> {
    let tpl = set(0, &template(256, &V9_FIELDS));
    let data = set(256, &v9_record([10, 0, 0, 1], [10, 0, 0, 2], 6, 22, 50_000, 100));
    vec![
        (Protocol::Netflow, v5(&[([10, 0, 0, 2], [1, 1, 1, 1], 1, 2, 6, 1, 1)], 0)),
        (Protocol::Netflow, v9(1, &[tpl.clone(), data.clone()])),
        (Protocol::Netflow, ipfix(1, &[tpl, data])),
        (Protocol::Sflow, sflow_sample_packet()),
    ]
}

fn decode_any(protocol: Protocol, data: &[u8], templates: &mut Templates) {
    match protocol {
        Protocol::Netflow => {
            let _ = netflow::decode(data, ip("10.0.0.1"), 0, templates);
        }
        Protocol::Sflow => {
            let _ = sflow::decode(data);
        }
    }
}

#[test]
fn truncated_flipped_and_random_input_never_panics() {
    let mut templates = Templates::default();
    let mut rng = Lcg(42);
    for (protocol, packet) in valid_packets() {
        for end in 0..packet.len() {
            decode_any(protocol, &packet[..end], &mut templates);
        }
        for _ in 0..2000 {
            let mut copy = packet.clone();
            for _ in 0..3 {
                let at = usize::from(rng.byte()) * copy.len() / 256;
                copy[at] = rng.byte();
            }
            decode_any(protocol, &copy, &mut templates);
        }
    }
    for version in [5u8, 9, 10] {
        for _ in 0..2000 {
            let len = usize::from(rng.byte()) * 4;
            let mut data: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
            if data.len() >= 2 {
                data[0] = 0;
                data[1] = version;
            }
            decode_any(Protocol::Netflow, &data, &mut templates);
            if data.len() >= 4 {
                data[..4].copy_from_slice(&[0, 0, 0, 5]);
            }
            decode_any(Protocol::Sflow, &data, &mut templates);
        }
    }
    let mut check = |data: &[u8]| netflow::decode(data, ip("10.0.0.1"), 0, &mut templates);
    assert_eq!(check(&[0u8, 7][..]), Err(DecodeError::Version));
    assert_eq!(check(&[0u8; 0][..]), Err(DecodeError::Malformed));
    // IPFIX : une longueur annoncée plus grande que le datagramme est refusée.
    let mut lying = ipfix(1, &[]);
    lying[3] = 200;
    assert_eq!(check(&lying[..]), Err(DecodeError::Malformed));
}

// -------------------------------------------------------------- agrégation

fn flow(src: &str, port: u16, bytes: u64) -> FlowRecord {
    FlowRecord::new(ip(src), ip("10.0.0.1"), 6, 50_000, port, bytes, 1)
}

#[test]
fn only_the_top_n_of_a_closed_minute_are_written_and_the_rest_is_other() {
    let mut aggregator = Aggregator::default();
    for n in 0..(TOP_N as u64 + 5) {
        let src = format!("192.168.0.{}", n + 1);
        aggregator.add(3, 100, &flow(&src, 443, 1000 + n));
        aggregator.add(3, 100, &flow(&src, 443, 1000 + n));
    }
    aggregator.add(3, 101, &flow("192.168.0.1", 22, 10));
    // La minute en cours n'est pas encore écrite.
    assert!(aggregator.drain(100).is_empty());
    let samples = aggregator.drain(101);
    let bytes: Vec<_> = samples.iter().filter(|s| s.metric == "flow_bytes_total").collect();
    assert_eq!(bytes.len(), TOP_N + 1);
    assert!(bytes.iter().all(|s| s.ts_ms == 100 * 60_000 && s.labels["target"] == "3"));
    // Le plus gros d'abord : 192.168.0.55 a envoyé deux fois 1054 octets.
    assert_eq!(bytes[0].labels["src"], "192.168.0.55");
    assert_eq!(bytes[0].value, 2108.0);
    assert_eq!(bytes[0].labels["proto"], "tcp");
    assert_eq!(bytes[0].labels["port"], "443");
    let other = bytes.iter().find(|s| s.labels["src"] == "other").unwrap();
    // Les cinq plus petits : 2 × (1000 + 1001 + 1002 + 1003 + 1004).
    assert_eq!(other.value, 10_020.0);
    let packets = samples.iter().filter(|s| s.metric == "flow_packets_total").count();
    assert_eq!(packets, TOP_N + 1);
    // La minute 101 attend sa fin.
    assert_eq!(aggregator.drain(102).len(), 2);
}

#[test]
fn the_global_series_cap_sends_new_exchanges_to_other() {
    let mut aggregator = Aggregator::default();
    // Autant de cibles que nécessaire, chacune à son plafond d'une minute.
    for target in 0..(MAX_SERIES / TOP_N) as i64 {
        for n in 0..TOP_N {
            aggregator.add(target, 0, &flow(&format!("10.0.{target}.{n}"), 80, 100));
        }
    }
    assert_eq!(aggregator.drain(1).len(), MAX_SERIES * 2);
    assert_eq!(aggregator.capped, 0);
    aggregator.add(1, 1, &flow("172.16.0.1", 80, 100));
    let samples = aggregator.drain(2);
    assert_eq!(aggregator.capped, 1);
    assert_eq!(samples.len(), 2);
    assert!(samples.iter().all(|s| s.labels["src"] == "other"));
}

// ------------------------------------------------------- filtre exportateurs

#[test]
fn only_monitored_exporters_are_read() {
    let hub = Hub::default();
    let now = Utc.with_ymd_and_hms(2026, 11, 28, 10, 0, 30).unwrap();
    let lookup = |from: IpAddr| (from == ip("10.0.0.1")).then(|| (5_i64, "router".to_string()));
    let packet = v5(&[([10, 0, 0, 2], [1, 1, 1, 1], 51_000, 443, 6, 10, 1500)], 0);

    hub.receive(Protocol::Netflow, &packet, ip("10.9.9.9"), now, lookup);
    hub.receive(Protocol::Netflow, &packet, ip("::ffff:10.0.0.1"), now, lookup);
    hub.receive(Protocol::Netflow, &[0, 5, 0], ip("10.0.0.1"), now, lookup);

    let counters = hub.counters();
    assert_eq!(counters.datagrams, 3);
    assert_eq!(counters.dropped_unknown, 1);
    assert_eq!(counters.dropped_malformed, 1);
    assert_eq!(counters.flows, 1);
    assert_eq!(hub.unknown_sources()[0].address, "10.9.9.9");
    let exporters = hub.exporters();
    assert_eq!(exporters.len(), 1);
    // Le datagramme illisible n'est pas compté pour l'exportateur.
    assert_eq!((exporters[0].target_id, exporters[0].datagrams), (5, 1));
    assert_eq!(exporters[0].address, "10.0.0.1");

    let later = Utc.with_ymd_and_hms(2026, 11, 28, 10, 1, 5).unwrap();
    let samples = hub.drain(later);
    assert_eq!(samples.len(), 2);
    assert!(samples.iter().all(|s| s.labels["target"] == "5"));
}

#[test]
fn settings_default_to_off_and_refuse_clashing_ports() {
    let settings = Settings::default();
    assert!(!settings.enabled);
    assert_eq!((settings.netflow_port, settings.sflow_port), (2055, 6343));
    assert!(settings.validate().is_ok());
    assert!(Settings { sflow_port: 2055, ..settings.clone() }.validate().is_err());
    assert!(Settings { netflow_port: 0, ..settings }.validate().is_err());
    let partial: Settings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
    assert_eq!(partial.sflow_port, 6343);
}
