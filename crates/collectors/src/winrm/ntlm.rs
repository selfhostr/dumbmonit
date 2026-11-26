//! NTLMv2 (MS-NLMP) pour l'authentification HTTP « Negotiate » de WinRM.
//!
//! Seule la poignée de main d'authentification est écrite : les trois messages
//! (Negotiate, Challenge, Authenticate). Ni signature ni chiffrement de session
//! (« sealing ») ne sont négociés : WinRM sur HTTPS protège déjà le canal, et sur
//! HTTP le serveur doit avoir autorisé explicitement le trafic non chiffré.
//!
//! MD4 (empreinte NT) vient de la crate `md4`, HMAC-MD5 est écrit ici par-dessus
//! `md-5` déjà présent (même approche que `uptime/radius`).

use md4::Md4;
use md5::Md5;

const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";

const NEGOTIATE_UNICODE: u32 = 0x0000_0001;
const REQUEST_TARGET: u32 = 0x0000_0004;
const NEGOTIATE_NTLM: u32 = 0x0000_0200;
const NEGOTIATE_EXTENDED_SESSIONSECURITY: u32 = 0x0008_0000;
const NEGOTIATE_TARGET_INFO: u32 = 0x0080_0000;
const NEGOTIATE_128: u32 = 0x2000_0000;
const NEGOTIATE_56: u32 = 0x8000_0000;

const FLAGS: u32 = NEGOTIATE_UNICODE
    | REQUEST_TARGET
    | NEGOTIATE_NTLM
    | NEGOTIATE_EXTENDED_SESSIONSECURITY
    | NEGOTIATE_TARGET_INFO
    | NEGOTIATE_128
    | NEGOTIATE_56;

/// Message 1 : l'annonce, sans domaine ni station.
pub fn negotiate_message() -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(SIGNATURE);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&FLAGS.to_le_bytes());
    // Domaine et station : champs vides pointant juste après l'en-tête.
    for _ in 0..2 {
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&32u32.to_le_bytes());
    }
    out
}

/// Ce que le message 2 (Challenge) apporte.
#[derive(Debug, PartialEq, Eq)]
pub struct Challenge {
    pub server_challenge: [u8; 8],
    pub target_info: Vec<u8>,
}

/// Lit le message 2. `None` s'il est tronqué ou n'est pas un message NTLM.
pub fn parse_challenge(message: &[u8]) -> Option<Challenge> {
    if message.len() < 48 || &message[..8] != SIGNATURE {
        return None;
    }
    if u32::from_le_bytes(message[8..12].try_into().ok()?) != 2 {
        return None;
    }
    let mut server_challenge = [0u8; 8];
    server_challenge.copy_from_slice(&message[24..32]);
    let len = u16::from_le_bytes(message[40..42].try_into().ok()?) as usize;
    let offset = u32::from_le_bytes(message[44..48].try_into().ok()?) as usize;
    let target_info = message.get(offset..offset.checked_add(len)?)?.to_vec();
    Some(Challenge { server_challenge, target_info })
}

/// Empreinte NT : MD4 du mot de passe en UTF-16LE.
pub fn nt_hash(password: &str) -> [u8; 16] {
    use md4::Digest;
    let mut hasher = Md4::new();
    hasher.update(utf16le(password));
    let mut out = [0u8; 16];
    let digest = hasher.finalize();
    out.copy_from_slice(AsRef::<[u8]>::as_ref(&digest));
    out
}

/// NTOWFv2 : HMAC-MD5(empreinte NT, UTF-16LE(MAJUSCULES(compte) + domaine)).
pub fn ntlmv2_hash(password: &str, user: &str, domain: &str) -> [u8; 16] {
    let mut identity = utf16le(&user.to_uppercase());
    identity.extend(utf16le(domain));
    hmac_md5(&nt_hash(password), &identity)
}

/// Le moment, en intervalles de 100 ns depuis 1601, que le protocole attend.
pub fn filetime(unix_seconds: i64) -> u64 {
    (unix_seconds.max(0) as u64 + 11_644_473_600) * 10_000_000
}

/// Valeur de la paire d'attributs `MsvAvTimestamp` (0x0007) des informations de
/// cible, si le serveur en a mis une : elle remplace l'heure locale.
fn server_timestamp(target_info: &[u8]) -> Option<[u8; 8]> {
    let mut rest = target_info;
    while rest.len() >= 4 {
        let id = u16::from_le_bytes([rest[0], rest[1]]);
        let len = u16::from_le_bytes([rest[2], rest[3]]) as usize;
        let value = rest.get(4..4 + len)?;
        if id == 0 {
            return None;
        }
        if id == 7 && len == 8 {
            return value.try_into().ok();
        }
        rest = &rest[4 + len..];
    }
    None
}

/// Message 3 : la preuve NTLMv2.
pub fn authenticate_message(
    challenge: &Challenge,
    user: &str,
    domain: &str,
    password: &str,
    client_challenge: [u8; 8],
    now_filetime: u64,
) -> Vec<u8> {
    let key = ntlmv2_hash(password, user, domain);
    let timestamp =
        server_timestamp(&challenge.target_info).unwrap_or_else(|| now_filetime.to_le_bytes());
    let mut temp = Vec::with_capacity(32 + challenge.target_info.len());
    temp.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0]);
    temp.extend_from_slice(&timestamp);
    temp.extend_from_slice(&client_challenge);
    temp.extend_from_slice(&[0; 4]);
    temp.extend_from_slice(&challenge.target_info);
    temp.extend_from_slice(&[0; 4]);

    let mut proof_input = challenge.server_challenge.to_vec();
    proof_input.extend_from_slice(&temp);
    let proof = hmac_md5(&key, &proof_input);
    let mut nt_response = proof.to_vec();
    nt_response.extend_from_slice(&temp);
    // Avec NTLMv2 et des informations de cible, la réponse LM est neutralisée.
    let lm_response = [0u8; 24];

    let domain_bytes = utf16le(domain);
    let user_bytes = utf16le(user);
    let empty: [u8; 0] = [];
    let fields: [&[u8]; 6] = [
        &lm_response,
        &nt_response,
        &domain_bytes,
        &user_bytes,
        &empty, // station
        &empty, // clé de session chiffrée
    ];
    // Ordre des champs dans l'en-tête : LM, NT, domaine, compte, station, clé.
    let mut out = Vec::new();
    out.extend_from_slice(SIGNATURE);
    out.extend_from_slice(&3u32.to_le_bytes());
    let mut offset = 64u32;
    for field in &fields {
        out.extend_from_slice(&(field.len() as u16).to_le_bytes());
        out.extend_from_slice(&(field.len() as u16).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += field.len() as u32;
    }
    out.extend_from_slice(&FLAGS.to_le_bytes());
    for field in &fields {
        out.extend_from_slice(field);
    }
    out
}

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn md5_of(parts: &[&[u8]]) -> [u8; 16] {
    use md5::Digest;
    let mut hasher = Md5::new();
    for part in parts {
        hasher.update(part);
    }
    let mut out = [0u8; 16];
    let digest = hasher.finalize();
    out.copy_from_slice(AsRef::<[u8]>::as_ref(&digest));
    out
}

/// HMAC-MD5 (RFC 2104).
fn hmac_md5(key: &[u8], message: &[u8]) -> [u8; 16] {
    let mut block = [0u8; 64];
    if key.len() > block.len() {
        block[..16].copy_from_slice(&md5_of(&[key]));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let inner_pad: Vec<u8> = block.iter().map(|byte| byte ^ 0x36).collect();
    let outer_pad: Vec<u8> = block.iter().map(|byte| byte ^ 0x5c).collect();
    let inner = md5_of(&[&inner_pad, message]);
    md5_of(&[&outer_pad, &inner])
}

#[cfg(test)]
mod tests;
