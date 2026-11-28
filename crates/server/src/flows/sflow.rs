//! sFlow v5 : échantillons de flux (simples et étendus).
//!
//! Chaque échantillon porte un en-tête de paquet brut (Ethernet, IPv4 ou
//! IPv6), ou un enregistrement « IPv4 / IPv6 échantillonné » déjà décodé par
//! l'équipement. Un échantillon représente `sampling_rate` paquets : octets et
//! paquets sont multipliés par ce taux. Les échantillons de compteurs sont
//! ignorés (les interfaces sont déjà suivies par SNMP).

use std::net::IpAddr;

use super::FlowRecord;
use super::cursor::Cursor;

/// Échantillons au plus par datagramme, enregistrements au plus par échantillon.
const MAX_SAMPLES: u32 = 256;
const MAX_RECORDS: u32 = 64;

/// Erreur de décodage : version inconnue ou datagramme incohérent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Version,
    Malformed,
}

pub fn decode(data: &[u8]) -> Result<Vec<FlowRecord>, DecodeError> {
    let mut c = Cursor::new(data);
    if c.u32().ok_or(DecodeError::Malformed)? != 5 {
        return Err(DecodeError::Version);
    }
    let mut records = Vec::new();
    datagram(&mut c, &mut records).ok_or(DecodeError::Malformed)?;
    Ok(records)
}

fn datagram(c: &mut Cursor, out: &mut Vec<FlowRecord>) -> Option<()> {
    match c.u32()? {
        1 => c.skip(4)?,
        2 => c.skip(16)?,
        _ => return None,
    }
    // sub_agent_id, sequence, uptime.
    c.skip(12)?;
    let samples = c.u32()?;
    if samples > MAX_SAMPLES {
        return None;
    }
    for _ in 0..samples {
        let format = c.u32()?;
        let len = usize::try_from(c.u32()?).ok()?;
        let mut body = c.sub(len)?;
        // Entreprise 0 seulement : 1 = échantillon de flux, 3 = étendu.
        match format {
            1 => flow_sample(&mut body, false, out)?,
            3 => flow_sample(&mut body, true, out)?,
            _ => {}
        }
    }
    Some(())
}

fn flow_sample(c: &mut Cursor, expanded: bool, out: &mut Vec<FlowRecord>) -> Option<()> {
    // sequence, puis source_id (un mot, ou type + index en étendu).
    c.skip(if expanded { 12 } else { 8 })?;
    let rate = u64::from(c.u32()?.max(1));
    // sample_pool, drops, puis input / output (un mot chacun, deux en étendu).
    c.skip(if expanded { 24 } else { 16 })?;
    let count = c.u32()?;
    if count > MAX_RECORDS {
        return None;
    }
    for _ in 0..count {
        let format = c.u32()?;
        let len = usize::try_from(c.u32()?).ok()?;
        let mut body = c.sub(len)?;
        let found = match format {
            1 => raw_header(&mut body),
            3 => sampled_ip(&mut body, false),
            4 => sampled_ip(&mut body, true),
            _ => None,
        };
        if let Some(record) = found {
            out.push(FlowRecord::new(
                record.src,
                record.dst,
                record.proto,
                record.src_port,
                record.dst_port,
                record.bytes.saturating_mul(rate),
                rate,
            ));
            // Un seul en-tête utile par échantillon : le paquet est le même.
            break;
        }
    }
    Some(())
}

/// Ce qu'un paquet échantillonné dit de lui-même (octets = taille de la trame).
struct Packet {
    src: IpAddr,
    dst: IpAddr,
    proto: u8,
    src_port: u16,
    dst_port: u16,
    bytes: u64,
}

fn raw_header(c: &mut Cursor) -> Option<Packet> {
    let protocol = c.u32()?;
    let frame_length = u64::from(c.u32()?);
    // stripped.
    c.skip(4)?;
    let header_length = usize::try_from(c.u32()?).ok()?;
    let header = c.take(header_length.min(c.remaining()))?;
    let mut packet = match protocol {
        1 => ethernet(header)?,
        11 => ipv4(header)?,
        12 => ipv6(header)?,
        _ => return None,
    };
    packet.bytes = frame_length;
    Some(packet)
}

fn sampled_ip(c: &mut Cursor, v6: bool) -> Option<Packet> {
    let bytes = u64::from(c.u32()?);
    let proto = u8::try_from(c.u32()?).ok()?;
    let (src, dst) = if v6 { (c.ipv6()?, c.ipv6()?) } else { (c.ipv4()?, c.ipv4()?) };
    let src_port = u16::try_from(c.u32()?).unwrap_or(0);
    let dst_port = u16::try_from(c.u32()?).unwrap_or(0);
    Some(Packet { src, dst, proto, src_port, dst_port, bytes })
}

/// Trame Ethernet : deux étiquettes VLAN au plus.
fn ethernet(frame: &[u8]) -> Option<Packet> {
    let mut c = Cursor::new(frame);
    c.skip(12)?;
    let mut ethertype = c.u16()?;
    for _ in 0..2 {
        if ethertype == 0x8100 || ethertype == 0x88A8 {
            c.skip(2)?;
            ethertype = c.u16()?;
        }
    }
    let rest = c.take(c.remaining())?;
    match ethertype {
        0x0800 => ipv4(rest),
        0x86DD => ipv6(rest),
        _ => None,
    }
}

fn ipv4(packet: &[u8]) -> Option<Packet> {
    let mut c = Cursor::new(packet);
    let first = c.u8()?;
    if first >> 4 != 4 {
        return None;
    }
    let header_len = usize::from(first & 0x0F) * 4;
    if header_len < 20 {
        return None;
    }
    // tos, total_length, identification.
    c.skip(5)?;
    let fragment = c.u16()? & 0x1FFF;
    // ttl.
    c.skip(1)?;
    let proto = c.u8()?;
    // checksum.
    c.skip(2)?;
    let src = c.ipv4()?;
    let dst = c.ipv4()?;
    // Un fragment autre que le premier ne porte pas l'en-tête de transport.
    let (src_port, dst_port) =
        if fragment == 0 { ports(packet.get(header_len..), proto) } else { (0, 0) };
    Some(Packet { src, dst, proto, src_port, dst_port, bytes: 0 })
}

fn ipv6(packet: &[u8]) -> Option<Packet> {
    let mut c = Cursor::new(packet);
    if c.u8()? >> 4 != 6 {
        return None;
    }
    // Reste de la classe et étiquette de flux, longueur de charge.
    c.skip(5)?;
    let proto = c.u8()?;
    // hop limit.
    c.skip(1)?;
    let src = c.ipv6()?;
    let dst = c.ipv6()?;
    let (src_port, dst_port) = ports(packet.get(40..), proto);
    Some(Packet { src, dst, proto, src_port, dst_port, bytes: 0 })
}

/// Ports TCP, UDP ou SCTP ; zéro pour les autres protocoles ou un en-tête coupé.
fn ports(transport: Option<&[u8]>, proto: u8) -> (u16, u16) {
    if !matches!(proto, 6 | 17 | 132) {
        return (0, 0);
    }
    let Some(bytes) = transport else { return (0, 0) };
    let mut c = Cursor::new(bytes);
    match (c.u16(), c.u16()) {
        (Some(src), Some(dst)) => (src, dst),
        _ => (0, 0),
    }
}
