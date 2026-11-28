//! NetFlow v5, NetFlow v9 (RFC 3954) et IPFIX (RFC 7011).
//!
//! Décodeurs minimaux : seuls les champs utiles aux « plus gros échanges »
//! sont lus (adresses, protocole, ports, octets, paquets, taux
//! d'échantillonnage). Tout est borné : longueurs vérifiées par [`Cursor`],
//! nombre de champs par gabarit, de gabarits par exportateur et au total ; un
//! datagramme mal formé rend une erreur, jamais une panique.
//!
//! Les gabarits v9 et IPFIX sont rangés par exportateur (adresse source),
//! version, domaine (`source_id` / `observation_domain_id`) et numéro, et
//! oubliés après [`TEMPLATE_TTL_SECS`] sans rafraîchissement.

use std::collections::HashMap;
use std::net::IpAddr;

use super::FlowRecord;
use super::cursor::{Cursor, be_uint, ip_of};

/// Champs au plus par gabarit.
pub const MAX_FIELDS: usize = 128;
/// Gabarits au plus par exportateur.
pub const MAX_TEMPLATES_PER_EXPORTER: usize = 256;
/// Gabarits au plus, tous exportateurs confondus.
pub const MAX_TEMPLATES: usize = 4096;
/// Un gabarit non renvoyé depuis trente minutes est oublié.
pub const TEMPLATE_TTL_SECS: u64 = 30 * 60;
/// Enregistrements au plus dans un datagramme v5 (limite du format).
const MAX_V5_RECORDS: usize = 30;
/// Longueur « variable » d'un champ IPFIX.
const VARIABLE: u16 = 0xFFFF;

/// Ce qu'un datagramme a donné.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Decoded {
    pub records: Vec<FlowRecord>,
    /// Ensembles de données dont le gabarit n'est pas (encore) connu.
    pub missing_template: u64,
    /// Version lue : 5, 9 ou 10 (IPFIX).
    pub version: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// Version inconnue : ni NetFlow v5/v9 ni IPFIX.
    Version,
    /// Datagramme tronqué ou incohérent.
    Malformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    exporter: IpAddr,
    version: u16,
    domain: u32,
    id: u16,
}

#[derive(Debug, Clone)]
struct Field {
    /// Numéro d'élément ; le bit 0x8000 marque un élément propre à un
    /// constructeur (IPFIX), jamais lu.
    id: u16,
    len: u16,
}

#[derive(Debug, Clone)]
struct Template {
    fields: Vec<Field>,
    /// Longueur minimale d'un enregistrement (un octet par champ variable).
    min_len: usize,
    seen: u64,
}

/// Gabarits connus, par exportateur.
#[derive(Debug, Default)]
pub struct Templates {
    map: HashMap<Key, Template>,
}

impl Templates {
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Oublie les gabarits périmés.
    pub fn expire(&mut self, now: u64) {
        self.map.retain(|_, template| now.saturating_sub(template.seen) < TEMPLATE_TTL_SECS);
    }

    /// Range un gabarit ; faux quand un plafond l'en empêche.
    fn insert(&mut self, key: Key, template: Template, now: u64) -> bool {
        if let Some(held) = self.map.get_mut(&key) {
            *held = template;
            return true;
        }
        if self.map.len() >= MAX_TEMPLATES {
            self.expire(now);
            if self.map.len() >= MAX_TEMPLATES {
                return false;
            }
        }
        let mine = self.map.keys().filter(|k| k.exporter == key.exporter).count();
        if mine >= MAX_TEMPLATES_PER_EXPORTER {
            return false;
        }
        self.map.insert(key, template);
        true
    }

    fn get(&self, key: &Key, now: u64) -> Option<&Template> {
        self.map.get(key).filter(|t| now.saturating_sub(t.seen) < TEMPLATE_TTL_SECS)
    }
}

/// Décode un datagramme NetFlow v5/v9 ou IPFIX reçu de `exporter` à `now`
/// (secondes, horloge quelconque mais monotone).
pub fn decode(
    data: &[u8],
    exporter: IpAddr,
    now: u64,
    templates: &mut Templates,
) -> Result<Decoded, DecodeError> {
    let version = Cursor::new(data).u16().ok_or(DecodeError::Malformed)?;
    match version {
        5 => v5(data).ok_or(DecodeError::Malformed),
        9 | 10 => templated(data, version, exporter, now, templates),
        _ => Err(DecodeError::Version),
    }
}

/// Multiplicateur d'un taux d'échantillonnage (0 ou 1 : aucun).
fn multiplier(rate: u64) -> u64 {
    rate.max(1)
}

fn v5(data: &[u8]) -> Option<Decoded> {
    let mut c = Cursor::new(data);
    c.skip(2)?;
    let count = usize::from(c.u16()?);
    if count == 0 || count > MAX_V5_RECORDS {
        return None;
    }
    // sys_uptime, unix_secs, unix_nsecs, flow_sequence, engine_type, engine_id.
    c.skip(18)?;
    // Deux bits de mode, quatorze bits d'intervalle.
    let rate = multiplier(u64::from(c.u16()? & 0x3FFF));
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let mut r = c.sub(48)?;
        let src = r.ipv4()?;
        let dst = r.ipv4()?;
        // nexthop, input, output.
        r.skip(8)?;
        let packets = u64::from(r.u32()?);
        let bytes = u64::from(r.u32()?);
        // first, last.
        r.skip(8)?;
        let src_port = r.u16()?;
        let dst_port = r.u16()?;
        // pad1, tcp_flags.
        r.skip(2)?;
        let proto = r.u8()?;
        if bytes == 0 {
            continue;
        }
        records.push(FlowRecord::new(
            src,
            dst,
            proto,
            src_port,
            dst_port,
            bytes.saturating_mul(rate),
            packets.saturating_mul(rate),
        ));
    }
    Some(Decoded { records, missing_template: 0, version: 5 })
}

fn templated(
    data: &[u8],
    version: u16,
    exporter: IpAddr,
    now: u64,
    templates: &mut Templates,
) -> Result<Decoded, DecodeError> {
    let ipfix = version == 10;
    let mut c = Cursor::new(data);
    let (length, domain) = header(&mut c, ipfix).ok_or(DecodeError::Malformed)?;
    if ipfix {
        // La longueur annoncée borne le message : ni plus court que l'en-tête,
        // ni plus long que le datagramme.
        if length < 16 || length > data.len() {
            return Err(DecodeError::Malformed);
        }
        c = Cursor::new(&data[16..length]);
    }
    // Les gabarits d'options (1 en v9, 3 en IPFIX) et les numéros réservés
    // sont ignorés.
    let template_set = if ipfix { 2 } else { 0 };
    let mut out = Decoded { version, ..Decoded::default() };
    while c.remaining() >= 4 {
        let set_id = c.u16().ok_or(DecodeError::Malformed)?;
        let set_len = usize::from(c.u16().ok_or(DecodeError::Malformed)?);
        if set_len < 4 {
            return Err(DecodeError::Malformed);
        }
        let Some(mut body) = c.sub(set_len - 4) else {
            // Ensemble tronqué : on garde ce qui précède s'il y a quelque chose.
            if out.records.is_empty() && out.missing_template == 0 {
                return Err(DecodeError::Malformed);
            }
            break;
        };
        if set_id == template_set {
            read_templates(&mut body, version, exporter, domain, now, templates);
        } else if set_id >= 256 {
            let key = Key { exporter, version, domain, id: set_id };
            let Some(template) = templates.get(&key, now).cloned() else {
                out.missing_template += 1;
                continue;
            };
            while body.remaining() >= template.min_len {
                match read_record(&mut body, &template, ipfix) {
                    Some(Some(record)) => out.records.push(record),
                    Some(None) => {}
                    None => break,
                }
            }
        }
    }
    Ok(out)
}

/// Longueur annoncée (IPFIX seulement) et domaine d'observation / `source_id`.
fn header(c: &mut Cursor, ipfix: bool) -> Option<(usize, u32)> {
    c.skip(2)?;
    if ipfix {
        // length, export_time, sequence, observation_domain_id.
        let length = usize::from(c.u16()?);
        c.skip(8)?;
        Some((length, c.u32()?))
    } else {
        // count, sys_uptime, unix_secs, sequence, source_id.
        c.skip(14)?;
        Some((0, c.u32()?))
    }
}

fn read_templates(
    body: &mut Cursor,
    version: u16,
    exporter: IpAddr,
    domain: u32,
    now: u64,
    templates: &mut Templates,
) {
    let ipfix = version == 10;
    while body.remaining() >= 4 {
        let (Some(id), Some(count)) = (body.u16(), body.u16()) else { return };
        let count = usize::from(count);
        if id < 256 {
            // Remplissage de fin d'ensemble, ou numéro réservé : on s'arrête.
            return;
        }
        let key = Key { exporter, version, domain, id };
        if count == 0 {
            // IPFIX : retrait d'un gabarit.
            templates.map.remove(&key);
            continue;
        }
        if count > MAX_FIELDS {
            return;
        }
        let mut fields = Vec::with_capacity(count);
        let mut min_len = 0usize;
        for _ in 0..count {
            let (Some(field_id), Some(len)) = (body.u16(), body.u16()) else { return };
            if ipfix && field_id & 0x8000 != 0 && body.u32().is_none() {
                return;
            }
            if len == 0 {
                return;
            }
            min_len += if ipfix && len == VARIABLE { 1 } else { usize::from(len) };
            fields.push(Field { id: field_id, len });
        }
        templates.insert(key, Template { fields, min_len, seen: now }, now);
    }
}

/// Un enregistrement de données ; `Some(None)` quand il ne décrit pas un
/// échange exploitable (pas d'adresses, aucun octet), `None` s'il est tronqué.
fn read_record(body: &mut Cursor, template: &Template, ipfix: bool) -> Option<Option<FlowRecord>> {
    let (mut src, mut dst) = (None, None);
    let (mut bytes, mut packets) = ([None; 3], [None; 3]);
    let (mut proto, mut src_port, mut dst_port, mut rate) = (0u8, 0u16, 0u16, 1u64);
    for field in &template.fields {
        let len = if ipfix && field.len == VARIABLE {
            match body.u8()? {
                255 => usize::from(body.u16()?),
                short => usize::from(short),
            }
        } else {
            usize::from(field.len)
        };
        let value = body.take(len)?;
        match field.id {
            // IN_BYTES / octetDeltaCount, OUT_BYTES, octetTotalCount.
            1 => bytes[0] = be_uint(value),
            23 => bytes[1] = be_uint(value),
            85 => bytes[2] = be_uint(value),
            // IN_PKTS / packetDeltaCount, OUT_PKTS, packetTotalCount.
            2 => packets[0] = be_uint(value),
            24 => packets[1] = be_uint(value),
            86 => packets[2] = be_uint(value),
            4 => proto = be_uint(value).unwrap_or(0) as u8,
            7 => src_port = be_uint(value).unwrap_or(0) as u16,
            11 => dst_port = be_uint(value).unwrap_or(0) as u16,
            8 | 27 => src = ip_of(value),
            12 | 28 => dst = ip_of(value),
            // SAMPLING_INTERVAL, samplingPacketInterval.
            34 | 305 => rate = multiplier(be_uint(value).unwrap_or(1)),
            _ => {}
        }
    }
    let bytes = bytes.into_iter().flatten().next().unwrap_or(0);
    let packets = packets.into_iter().flatten().next().unwrap_or(0);
    let (Some(src), Some(dst)) = (src, dst) else { return Some(None) };
    if bytes == 0 {
        return Some(None);
    }
    Some(Some(FlowRecord::new(
        src,
        dst,
        proto,
        src_port,
        dst_port,
        bytes.saturating_mul(rate),
        packets.saturating_mul(rate),
    )))
}
