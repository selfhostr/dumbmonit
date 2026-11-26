//! Signature AWS Version 4 (`AWS4-HMAC-SHA256`), en en-têtes.
//!
//! Juste ce qu'il faut pour parler à S3 et à ses compatibles (MinIO,
//! Backblaze B2, Wasabi, Cloudflare R2) : requête canonique, chaîne à signer,
//! clé de signature dérivée par HMAC en cascade. Pas de normalisation de
//! chemin (S3 n'en fait pas) : le chemin canonique est celui qui part sur le
//! fil, chaque segment encodé une fois par l'appelant ([`uri_encode`]).
//!
//! Les tests rejouent les vecteurs publiés par AWS (suite de signature
//! `aws-signing-test-suite`, et l'exemple « GET Object » de la documentation
//! S3) : une erreur d'un caractère dans la forme canonique s'y voit.

use std::collections::BTreeMap;

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// Empreinte SHA-256 d'un corps vide.
pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Encode selon les règles d'AWS : seuls `A-Z a-z 0-9 - _ . ~` restent tels
/// quels, tout le reste devient `%XX` (majuscules), octet par octet. `/` est
/// gardé quand `encode_slash` est faux (chemin d'objet).
pub fn uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (byte == b'/' && !encode_slash);
        if keep {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    // Conversion directe du condensat : pas de tampon intermédiaire initialisé à zéro.
    mac.finalize().into_bytes().into()
}

/// Ce qui est signé d'une requête.
pub struct Request<'a> {
    pub method: &'a str,
    /// Chemin tel qu'il part sur le fil, déjà encodé.
    pub canonical_uri: &'a str,
    /// Paramètres de requête, **décodés** : ils sont encodés ici.
    pub query: &'a [(String, String)],
    /// En-têtes signés, `host` et `x-amz-date` compris.
    pub headers: &'a [(String, String)],
    /// SHA-256 hexadécimal du corps (ou `UNSIGNED-PAYLOAD`).
    pub payload_hash: &'a str,
}

/// Chaîne de requête canonique : clés et valeurs encodées, triées.
pub fn canonical_query(query: &[(String, String)]) -> String {
    let mut pairs: Vec<(String, String)> =
        query.iter().map(|(k, v)| (uri_encode(k, true), uri_encode(v, true))).collect();
    pairs.sort();
    pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

/// Valeur d'en-tête canonique : bords retirés, espaces internes réduits à un.
fn canonical_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Requête canonique et liste des en-têtes signés (`host;x-amz-date`).
pub fn canonical_request(request: &Request<'_>) -> (String, String) {
    let mut headers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, value) in request.headers {
        headers.entry(name.trim().to_ascii_lowercase()).or_default().push(canonical_value(value));
    }
    let canonical_headers: String =
        headers.iter().map(|(name, values)| format!("{name}:{}\n", values.join(","))).collect();
    let signed = headers.keys().cloned().collect::<Vec<_>>().join(";");
    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        request.method,
        request.canonical_uri,
        canonical_query(request.query),
        canonical_headers,
        signed,
        request.payload_hash
    );
    (canonical, signed)
}

/// `AAAAMMJJ/région/service/aws4_request`.
pub fn credential_scope(date: &str, region: &str, service: &str) -> String {
    format!("{date}/{region}/{service}/aws4_request")
}

pub fn string_to_sign(amz_date: &str, scope: &str, canonical_request: &str) -> String {
    format!("{ALGORITHM}\n{amz_date}\n{scope}\n{}", sha256_hex(canonical_request.as_bytes()))
}

/// Clé du jour, de la région et du service.
pub fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> [u8; 32] {
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    hmac(&k_service, b"aws4_request")
}

/// Identifiants et portée d'une signature.
pub struct Signer<'a> {
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub region: &'a str,
    pub service: &'a str,
}

impl Signer<'_> {
    /// Signature hexadécimale de `request` à l'instant `amz_date`
    /// (`AAAAMMJJTHHMMSSZ`).
    pub fn signature(&self, request: &Request<'_>, amz_date: &str) -> (String, String) {
        let date = &amz_date[..amz_date.len().min(8)];
        let scope = credential_scope(date, self.region, self.service);
        let (canonical, signed) = canonical_request(request);
        let to_sign = string_to_sign(amz_date, &scope, &canonical);
        let key = signing_key(self.secret_key, date, self.region, self.service);
        (hex::encode(hmac(&key, to_sign.as_bytes())), signed)
    }

    /// Valeur de l'en-tête `Authorization`.
    pub fn authorization(&self, request: &Request<'_>, amz_date: &str) -> String {
        let date = &amz_date[..amz_date.len().min(8)];
        let scope = credential_scope(date, self.region, self.service);
        let (signature, signed) = self.signature(request, amz_date);
        format!(
            "{ALGORITHM} Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}",
            self.access_key
        )
    }
}

#[cfg(test)]
mod tests;
