//! Lecture bornée d'un datagramme : chaque accès vérifie la longueur restante
//! et rend `None` plutôt que de paniquer. Les décodeurs NetFlow, IPFIX et sFlow
//! ne lisent les octets reçus qu'à travers ce curseur.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy)]
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Les `n` octets suivants, et le curseur avance.
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    /// Un sous-curseur sur les `n` octets suivants.
    pub fn sub(&mut self, n: usize) -> Option<Cursor<'a>> {
        self.take(n).map(Cursor::new)
    }

    pub fn skip(&mut self, n: usize) -> Option<()> {
        self.take(n).map(|_| ())
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    pub fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn ipv4(&mut self) -> Option<IpAddr> {
        self.take(4).map(|b| IpAddr::V4(Ipv4Addr::new(b[0], b[1], b[2], b[3])))
    }

    pub fn ipv6(&mut self) -> Option<IpAddr> {
        let bytes: [u8; 16] = self.take(16)?.try_into().ok()?;
        Some(IpAddr::V6(Ipv6Addr::from(bytes)))
    }
}

/// Entier non signé big-endian de 1 à 8 octets ; `None` au-delà.
pub fn be_uint(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    Some(bytes.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b)))
}

/// Adresse IP d'un champ de 4 ou 16 octets.
pub fn ip_of(bytes: &[u8]) -> Option<IpAddr> {
    match bytes.len() {
        4 => Cursor::new(bytes).ipv4(),
        16 => Cursor::new(bytes).ipv6(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_past_the_end_are_refused() {
        let mut cursor = Cursor::new(&[0, 1, 2]);
        assert_eq!(cursor.u16(), Some(1));
        assert_eq!(cursor.u16(), None);
        assert_eq!(cursor.u8(), Some(2));
        assert_eq!(cursor.take(usize::MAX), None);
        assert_eq!(be_uint(&[1, 0]), Some(256));
        assert_eq!(be_uint(&[0; 9]), None);
        assert_eq!(ip_of(&[10, 0, 0, 1]), Some("10.0.0.1".parse().unwrap()));
        assert_eq!(ip_of(&[1, 2]), None);
    }
}
