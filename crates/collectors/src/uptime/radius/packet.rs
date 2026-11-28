//! Paquets RADIUS (RFC 2865) : encodage d'un `Access-Request` et lecture de la
//! réponse, sans dépendance au-delà de MD5.
//!
//! Tout ce que la sonde échange tient en quelques attributs. Le secret partagé
//! n'est jamais transmis : il sert à masquer le mot de passe (RFC 2865 §5.2), à
//! signer la requête (`Message-Authenticator`, RFC 3579) et à vérifier que la
//! réponse vient bien d'un serveur qui le connaît (`Response Authenticator`,
//! RFC 2865 §3). C'est ce dernier contrôle qui permet de dire « le secret est
//! faux » plutôt que « le serveur est en panne ».

use md5::{Digest, Md5};

pub const ACCESS_REQUEST: u8 = 1;
pub const ACCESS_ACCEPT: u8 = 2;
pub const ACCESS_REJECT: u8 = 3;
pub const ACCESS_CHALLENGE: u8 = 11;

const USER_NAME: u8 = 1;
const USER_PASSWORD: u8 = 2;
const REPLY_MESSAGE: u8 = 18;
const NAS_IDENTIFIER: u8 = 32;
const MESSAGE_AUTHENTICATOR: u8 = 80;

/// Code, identifiant, longueur (2 octets) et authentificateur (16 octets).
const HEADER_LEN: usize = 20;
/// Taille maximale d'un paquet (RFC 2865 §3).
const MAX_PACKET: usize = 4096;
/// Un mot de passe masqué tient en 128 octets au plus (RFC 2865 §5.2).
pub const MAX_PASSWORD: usize = 128;
/// La valeur d'un attribut tient en 253 octets (la longueur est sur un octet).
pub const MAX_VALUE: usize = 253;

/// Pourquoi une réponse n'a pas été retenue.
#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Ce n'est pas un paquet RADIUS bien formé.
    Malformed(String),
    /// L'identifiant ne correspond pas à la requête : réponse à une autre, ou
    /// paquet qui n'a rien à voir.
    IdMismatch,
    /// La signature ne se vérifie pas avec notre secret : le secret partagé est
    /// faux, ou le paquet a été fabriqué.
    BadAuthenticator(String),
}

/// Une réponse lue et authentifiée.
#[derive(Debug)]
pub struct Response {
    pub code: u8,
    pub attributes: Vec<(u8, Vec<u8>)>,
}

impl Response {
    /// Le `Reply-Message` du serveur, nettoyé et borné, pour l'afficher dans le
    /// détail d'un échec.
    pub fn reply_message(&self) -> Option<String> {
        let (_, value) = self.attributes.iter().find(|(kind, _)| *kind == REPLY_MESSAGE)?;
        let text: String = String::from_utf8_lossy(value)
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .take(120)
            .collect();
        let text = text.trim().to_string();
        if text.is_empty() { None } else { Some(text) }
    }
}

/// Un attribut tel qu'il se trouve dans le paquet.
struct Attribute<'a> {
    kind: u8,
    value: &'a [u8],
    /// Position de la valeur dans le paquet.
    offset: usize,
}

/// Découpe les attributs qui suivent l'en-tête.
fn parse_attributes(packet: &[u8]) -> Result<Vec<Attribute<'_>>, String> {
    let mut attributes = Vec::new();
    let mut position = HEADER_LEN;
    while position < packet.len() {
        if position + 2 > packet.len() {
            return Err("attribute header cut short".to_string());
        }
        let kind = packet[position];
        let length = usize::from(packet[position + 1]);
        if length < 2 || position + length > packet.len() {
            return Err(format!("attribute {kind} has an invalid length ({length})"));
        }
        attributes.push(Attribute {
            kind,
            value: &packet[position + 2..position + length],
            offset: position + 2,
        });
        position += length;
    }
    Ok(attributes)
}

fn md5(parts: &[&[u8]]) -> [u8; 16] {
    let mut hasher = Md5::new();
    for &part in parts {
        hasher.update(part);
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&hasher.finalize());
    out
}

/// HMAC-MD5 (RFC 2104), écrit à la main : la signature `Message-Authenticator`
/// est la seule chose qui en a besoin, et MD5 est déjà là.
pub(super) fn hmac_md5(key: &[u8], message: &[u8]) -> [u8; 16] {
    let mut block = [0u8; 64];
    if key.len() > block.len() {
        block[..16].copy_from_slice(&md5(&[key]));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let inner_pad: Vec<u8> = block.iter().map(|byte| byte ^ 0x36).collect();
    let outer_pad: Vec<u8> = block.iter().map(|byte| byte ^ 0x5c).collect();
    let inner = md5(&[&inner_pad, message]);
    md5(&[&outer_pad, &inner])
}

/// Masque le mot de passe (RFC 2865 §5.2) : complété par des zéros jusqu'à un
/// multiple de 16, puis chaque bloc est combiné par XOR avec
/// `MD5(secret + bloc chiffré précédent)`, le premier avec l'authentificateur.
fn hide_password(password: &[u8], secret: &[u8], authenticator: &[u8; 16]) -> Vec<u8> {
    let padded_len = password.len().div_ceil(16).max(1) * 16;
    let mut plain = password.to_vec();
    plain.resize(padded_len, 0);

    let mut hidden = Vec::with_capacity(padded_len);
    let mut previous: Vec<u8> = authenticator.to_vec();
    for chunk in plain.chunks(16) {
        let key = md5(&[secret, &previous]);
        let cipher: Vec<u8> = chunk.iter().zip(key).map(|(plain, key)| plain ^ key).collect();
        hidden.extend_from_slice(&cipher);
        previous = cipher;
    }
    hidden
}

/// Opération inverse de [`hide_password`], pour le serveur factice des tests.
#[cfg(test)]
pub(super) fn reveal_password(hidden: &[u8], secret: &[u8], authenticator: &[u8; 16]) -> Vec<u8> {
    let mut plain = Vec::with_capacity(hidden.len());
    let mut previous: Vec<u8> = authenticator.to_vec();
    for chunk in hidden.chunks(16) {
        let key = md5(&[secret, &previous]);
        plain.extend(chunk.iter().zip(key).map(|(cipher, key)| cipher ^ key));
        previous = chunk.to_vec();
    }
    while plain.last() == Some(&0) {
        plain.pop();
    }
    plain
}

fn push_attribute(packet: &mut Vec<u8>, kind: u8, value: &[u8]) {
    packet.push(kind);
    packet.push((value.len() + 2) as u8);
    packet.extend_from_slice(value);
}

/// Construit un `Access-Request`.
///
/// Le `Message-Authenticator` vient en premier : c'est la parade recommandée
/// contre l'attaque « BlastRADIUS » (CVE-2024-3596), et un serveur à jour
/// exige désormais cet attribut.
pub fn encode_access_request(
    id: u8,
    authenticator: &[u8; 16],
    secret: &[u8],
    username: &str,
    password: &str,
    nas_identifier: &str,
) -> Result<Vec<u8>, String> {
    if secret.is_empty() {
        return Err("the shared secret is empty".to_string());
    }
    if username.is_empty() || username.len() > MAX_VALUE {
        return Err(format!("the user name must hold 1 to {MAX_VALUE} bytes"));
    }
    if password.len() > MAX_PASSWORD {
        return Err(format!("the password is limited to {MAX_PASSWORD} bytes by RADIUS"));
    }
    if nas_identifier.len() > MAX_VALUE {
        return Err(format!("the NAS identifier is limited to {MAX_VALUE} bytes"));
    }

    let mut packet = vec![ACCESS_REQUEST, id, 0, 0];
    packet.extend_from_slice(authenticator);

    let signature_offset = packet.len() + 2;
    push_attribute(&mut packet, MESSAGE_AUTHENTICATOR, &[0u8; 16]);
    push_attribute(&mut packet, USER_NAME, username.as_bytes());
    push_attribute(
        &mut packet,
        USER_PASSWORD,
        &hide_password(password.as_bytes(), secret, authenticator),
    );
    if !nas_identifier.is_empty() {
        push_attribute(&mut packet, NAS_IDENTIFIER, nas_identifier.as_bytes());
    }

    let length = (packet.len() as u16).to_be_bytes();
    packet[2..4].copy_from_slice(&length);
    let signature = hmac_md5(secret, &packet);
    packet[signature_offset..signature_offset + 16].copy_from_slice(&signature);
    Ok(packet)
}

/// Lit et authentifie une réponse.
///
/// Les contrôles vont du plus anodin au plus parlant : paquet mal formé,
/// identifiant d'une autre requête, puis signatures. Une signature qui ne
/// correspond pas veut presque toujours dire que le secret partagé est faux.
pub fn decode_response(
    datagram: &[u8],
    request_authenticator: &[u8; 16],
    expected_id: u8,
    secret: &[u8],
) -> Result<Response, DecodeError> {
    let malformed = |detail: &str| DecodeError::Malformed(detail.to_string());
    if datagram.len() < HEADER_LEN {
        return Err(malformed("shorter than a RADIUS header"));
    }
    let length = usize::from(u16::from_be_bytes([datagram[2], datagram[3]]));
    if !(HEADER_LEN..=MAX_PACKET).contains(&length) || length > datagram.len() {
        return Err(malformed("the length field does not match the datagram"));
    }
    // Les octets au-delà de la longueur annoncée sont du remplissage (RFC 2865 §3).
    let packet = &datagram[..length];
    let attributes = parse_attributes(packet).map_err(DecodeError::Malformed)?;

    if packet[1] != expected_id {
        return Err(DecodeError::IdMismatch);
    }

    let expected = md5(&[&packet[..4], request_authenticator, &packet[HEADER_LEN..], secret]);
    if packet[4..HEADER_LEN] != expected {
        return Err(DecodeError::BadAuthenticator(
            "the Response Authenticator does not match".to_string(),
        ));
    }

    if let Some(signature) = attributes.iter().find(|a| a.kind == MESSAGE_AUTHENTICATOR) {
        if signature.value.len() != 16 {
            return Err(malformed("the Message-Authenticator is not 16 bytes long"));
        }
        let mut copy = packet.to_vec();
        copy[4..HEADER_LEN].copy_from_slice(request_authenticator);
        copy[signature.offset..signature.offset + 16].fill(0);
        if hmac_md5(secret, &copy) != signature.value {
            return Err(DecodeError::BadAuthenticator(
                "the Message-Authenticator does not match".to_string(),
            ));
        }
    }

    Ok(Response {
        code: packet[0],
        attributes: attributes.iter().map(|a| (a.kind, a.value.to_vec())).collect(),
    })
}

/// Ce qu'un serveur RADIUS factice lit dans une requête, pour les tests.
#[cfg(test)]
pub(super) struct ParsedRequest {
    pub id: u8,
    pub authenticator: [u8; 16],
    pub username: String,
    pub password: String,
    pub nas_identifier: String,
    /// La requête commence-t-elle par un `Message-Authenticator` valide ?
    pub signature_valid: bool,
    pub signature_first: bool,
}

#[cfg(test)]
pub(super) fn parse_request(packet: &[u8], secret: &[u8]) -> Result<ParsedRequest, String> {
    if packet.len() < HEADER_LEN || packet[0] != ACCESS_REQUEST {
        return Err("not an Access-Request".to_string());
    }
    let attributes = parse_attributes(packet)?;
    let mut authenticator = [0u8; 16];
    authenticator.copy_from_slice(&packet[4..HEADER_LEN]);

    let text = |kind: u8| {
        attributes
            .iter()
            .find(|a| a.kind == kind)
            .map(|a| String::from_utf8_lossy(a.value).into_owned())
            .unwrap_or_default()
    };
    let hidden = attributes.iter().find(|a| a.kind == USER_PASSWORD).map_or(&[][..], |a| a.value);
    let revealed = reveal_password(hidden, secret, &authenticator);

    let signature = attributes.iter().find(|a| a.kind == MESSAGE_AUTHENTICATOR);
    let signature_valid = signature.is_some_and(|signature| {
        let mut copy = packet.to_vec();
        copy[signature.offset..signature.offset + 16].fill(0);
        hmac_md5(secret, &copy) == signature.value
    });
    Ok(ParsedRequest {
        id: packet[1],
        authenticator,
        username: text(USER_NAME),
        password: String::from_utf8_lossy(&revealed).into_owned(),
        nas_identifier: text(NAS_IDENTIFIER),
        signature_valid,
        signature_first: attributes.first().is_some_and(|a| a.kind == MESSAGE_AUTHENTICATOR),
    })
}

/// Construit une réponse signée par `secret`, pour les tests.
#[cfg(test)]
pub(super) fn build_response(
    code: u8,
    id: u8,
    request_authenticator: &[u8; 16],
    secret: &[u8],
    attributes: &[(u8, Vec<u8>)],
) -> Vec<u8> {
    let mut packet = vec![code, id, 0, 0];
    packet.extend_from_slice(&[0u8; 16]);
    for (kind, value) in attributes {
        push_attribute(&mut packet, *kind, value);
    }
    let length = (packet.len() as u16).to_be_bytes();
    packet[2..4].copy_from_slice(&length);
    let signature = md5(&[&packet[..4], request_authenticator, &packet[HEADER_LEN..], secret]);
    packet[4..HEADER_LEN].copy_from_slice(&signature);
    packet
}

#[cfg(test)]
mod tests;
