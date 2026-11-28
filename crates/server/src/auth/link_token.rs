//! Jetons des liens à usage unique envoyés par courriel : réinitialisation du
//! mot de passe et invitation.
//!
//! Même forme que le cookie de session (`<identifiant>.<secret>`) et pour les
//! mêmes raisons : l'identifiant public retrouve la ligne, le secret de 256 bits
//! la prouve, et seule l'empreinte SHA-256 du secret est stockée. La
//! comparaison se fait en temps constant. Le jeton ne passe jamais par un
//! journal : `Debug` est écrit à la main, et le serveur ne le reçoit que dans
//! un corps JSON (le chemin, seul journalisé, n'en porte aucun).

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Octets de l'identifiant public.
const ID_BYTES: usize = 16;
/// Octets du secret : 256 bits, hors de portée de toute recherche.
const SECRET_BYTES: usize = 32;

#[derive(Clone)]
pub struct LinkToken {
    id: String,
    secret: String,
}

impl std::fmt::Debug for LinkToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LinkToken {{ id: {:?}, secret: <redacted> }}", self.id)
    }
}

impl LinkToken {
    /// Tire un jeton neuf.
    pub fn generate() -> Self {
        Self {
            id: hex::encode(rand::random::<[u8; ID_BYTES]>()),
            secret: hex::encode(rand::random::<[u8; SECRET_BYTES]>()),
        }
    }

    /// Relit un jeton saisi ou collé. Toute autre forme que deux blocs
    /// hexadécimaux de la bonne longueur est refusée sans détail.
    pub fn parse(raw: &str) -> Option<Self> {
        let (id, secret) = raw.trim().split_once('.')?;
        let hex = |part: &str, bytes: usize| {
            part.len() == bytes * 2 && part.bytes().all(|b| b.is_ascii_hexdigit())
        };
        (hex(id, ID_BYTES) && hex(secret, SECRET_BYTES))
            .then(|| Self { id: id.to_ascii_lowercase(), secret: secret.to_ascii_lowercase() })
    }

    /// Partie publique, qui retrouve la ligne en base.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Valeur à mettre dans le lien.
    pub fn value(&self) -> String {
        format!("{}.{}", self.id, self.secret)
    }

    /// Empreinte du secret, telle qu'elle est stockée.
    pub fn secret_hash(&self) -> Vec<u8> {
        Sha256::digest(self.secret.as_bytes()).to_vec()
    }

    /// Le secret correspond-il à l'empreinte stockée ? En temps constant.
    pub fn matches(&self, stored: &[u8]) -> bool {
        bool::from(self.secret_hash().as_slice().ct_eq(stored))
    }
}

#[cfg(test)]
mod tests;
