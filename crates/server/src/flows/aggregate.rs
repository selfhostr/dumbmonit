//! Agrégation en mémoire : par exportateur (cible) et par minute, octets et
//! paquets par (source, destination, protocole, port).
//!
//! À la fin de chaque minute, seuls les [`TOP_N`] plus gros échanges deviennent
//! des séries ; le reste est cumulé dans une série « other ». Deux gardes de
//! cardinalité s'ajoutent : un plafond de clés distinctes en mémoire (une
//! inondation de faux flux ne fait pas grossir la table), et un plafond global
//! de séries écrites sur la dernière heure ([`MAX_SERIES`]) ; au-delà, un
//! échange nouveau va lui aussi dans « other ».
//!
//! La valeur écrite est le volume de la minute (pas un compteur cumulé) :
//! `sum_over_time` sur une fenêtre donne le volume de la fenêtre.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;

use dumbmonit_proto::{MetricKind, Sample};

use super::FlowRecord;

/// Échanges gardés par exportateur et par minute.
pub const TOP_N: usize = 50;
/// Clés distinctes au plus dans une minute d'un exportateur.
pub const MAX_KEYS_PER_BUCKET: usize = 20_000;
/// Clés distinctes au plus en mémoire, tous exportateurs confondus.
pub const MAX_KEYS: usize = 200_000;
/// Séries distinctes écrites au plus sur la dernière heure.
pub const MAX_SERIES: usize = 10_000;
const SERIES_WINDOW_MINUTES: i64 = 60;

pub const BYTES_METRIC: &str = "flow_bytes_total";
pub const PACKETS_METRIC: &str = "flow_packets_total";
/// Valeur des étiquettes de la série qui cumule le reste.
pub const OTHER: &str = "other";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Tuple {
    src: IpAddr,
    dst: IpAddr,
    proto: u8,
    port: u16,
}

#[derive(Debug, Default, Clone, Copy)]
struct Volume {
    bytes: u64,
    packets: u64,
}

impl Volume {
    fn add(&mut self, other: Volume) {
        self.bytes = self.bytes.saturating_add(other.bytes);
        self.packets = self.packets.saturating_add(other.packets);
    }
}

#[derive(Debug, Default)]
struct Bucket {
    tuples: HashMap<Tuple, Volume>,
    other: Volume,
}

#[derive(Debug, Default)]
pub struct Aggregator {
    /// Par (minute, cible).
    buckets: BTreeMap<(i64, i64), Bucket>,
    keys: usize,
    /// Séries écrites récemment, et la dernière minute où elles l'ont été.
    series: HashMap<String, i64>,
    /// Échanges renvoyés dans « other » par le plafond global de séries.
    pub capped: u64,
}

/// Nom court d'un protocole IP, sinon son numéro.
pub fn proto_name(proto: u8) -> String {
    match proto {
        1 => "icmp".into(),
        6 => "tcp".into(),
        17 => "udp".into(),
        47 => "gre".into(),
        50 => "esp".into(),
        58 => "icmpv6".into(),
        132 => "sctp".into(),
        other => other.to_string(),
    }
}

impl Aggregator {
    /// Compte un échange de `target`, reçu pendant `minute` (secondes / 60).
    pub fn add(&mut self, target: i64, minute: i64, record: &FlowRecord) {
        let volume = Volume { bytes: record.bytes, packets: record.packets };
        let tuple =
            Tuple { src: record.src, dst: record.dst, proto: record.proto, port: record.port };
        let full = self.keys >= MAX_KEYS;
        let bucket = self.buckets.entry((minute, target)).or_default();
        if let Some(held) = bucket.tuples.get_mut(&tuple) {
            held.add(volume);
        } else if !full && bucket.tuples.len() < MAX_KEYS_PER_BUCKET {
            bucket.tuples.insert(tuple, volume);
            self.keys += 1;
        } else {
            bucket.other.add(volume);
        }
    }

    /// Les minutes closes (antérieures à `current_minute`), en échantillons.
    pub fn drain(&mut self, current_minute: i64) -> Vec<Sample> {
        let closed: Vec<(i64, i64)> =
            self.buckets.range(..(current_minute, i64::MIN)).map(|(key, _)| *key).collect();
        let mut out = Vec::new();
        for key in closed {
            if let Some(bucket) = self.buckets.remove(&key) {
                self.keys = self.keys.saturating_sub(bucket.tuples.len());
                self.emit(key.1, key.0, bucket, &mut out);
            }
        }
        self.series.retain(|_, last| current_minute - *last <= SERIES_WINDOW_MINUTES);
        out
    }

    fn emit(&mut self, target: i64, minute: i64, bucket: Bucket, out: &mut Vec<Sample>) {
        let mut rows: Vec<(Tuple, Volume)> = bucket.tuples.into_iter().collect();
        rows.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then_with(|| a.0.cmp(&b.0)));
        let mut other = bucket.other;
        let ts_ms = minute.saturating_mul(60_000);
        for (rank, (tuple, volume)) in rows.into_iter().enumerate() {
            if rank >= TOP_N {
                other.add(volume);
                continue;
            }
            let labels = [
                tuple.src.to_canonical().to_string(),
                tuple.dst.to_canonical().to_string(),
                proto_name(tuple.proto),
                tuple.port.to_string(),
            ];
            let key = format!("{target}|{}", labels.join("|"));
            if !self.series.contains_key(&key) && self.series.len() >= MAX_SERIES {
                self.capped += 1;
                other.add(volume);
                continue;
            }
            self.series.insert(key, minute);
            push(out, target, &labels, volume, ts_ms);
        }
        if other.bytes > 0 {
            let labels = [OTHER; 4].map(String::from);
            push(out, target, &labels, other, ts_ms);
        }
    }
}

fn push(out: &mut Vec<Sample>, target: i64, labels: &[String; 4], volume: Volume, ts_ms: i64) {
    for (metric, value) in [(BYTES_METRIC, volume.bytes), (PACKETS_METRIC, volume.packets)] {
        // Volume de la minute : une jauge, lue par `sum_over_time`.
        let sample = Sample::new(metric, value as f64, MetricKind::Gauge, ts_ms)
            .with_label("target", target.to_string())
            .with_label("src", labels[0].clone())
            .with_label("dst", labels[1].clone())
            .with_label("proto", labels[2].clone())
            .with_label("port", labels[3].clone());
        out.push(sample);
    }
}
