//! Lecture bornée du JSON venu de l'extérieur (corps de requête, réglages
//! soumis, fichier de sauvegarde, document GitOps).
//!
//! `serde_json` ne lit que des données — aucun type n'est instancié au gré du
//! document — mais un document hostile peut encore coûter cher : très gros, ou
//! imbriqué au point d'épuiser la pile. Tout JSON non fiable passe donc par
//! ici : taille et profondeur sont vérifiées **avant** la désérialisation vers
//! le type attendu, et un champ inconnu reste refusé là où le type le demande
//! (`deny_unknown_fields`).

use std::fmt;

use serde::de::DeserializeOwned;
use serde_json::Value;

/// Taille maximale d'un document lu ici : le plafond global des corps de l'API.
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
/// Profondeur d'imbrication maximale (objets et tableaux confondus).
pub const MAX_DEPTH: usize = 32;

/// Pourquoi un document a été refusé.
#[derive(Debug)]
pub enum Rejected {
    TooLarge,
    TooDeep,
    Invalid(serde_json::Error),
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "document larger than {} KiB", MAX_BYTES / 1024),
            Self::TooDeep => write!(f, "document nested deeper than {MAX_DEPTH} levels"),
            Self::Invalid(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Rejected {}

/// Décode des octets non fiables vers `T`, après contrôle de taille et de profondeur.
pub fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Rejected> {
    check_bytes(bytes)?;
    serde_json::from_slice(bytes).map_err(Rejected::Invalid)
}

/// Convertit une valeur déjà décodée (champ libre d'un corps) vers `T`, après
/// contrôle de profondeur. Sa taille est déjà bornée par le corps qui la portait.
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, Rejected> {
    check_value(&value)?;
    serde_json::from_value(value).map_err(Rejected::Invalid)
}

/// Contrôle des octets bruts, sans rien allouer : longueur, puis profondeur des
/// `{`/`[` hors chaînes de caractères.
pub fn check_bytes(bytes: &[u8]) -> Result<(), Rejected> {
    if bytes.len() > MAX_BYTES {
        return Err(Rejected::TooLarge);
    }
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for &byte in bytes {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(Rejected::TooDeep);
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// Contrôle d'une valeur décodée, parcourue sans récursion.
pub fn check_value(value: &Value) -> Result<(), Rejected> {
    let mut pending = vec![(value, 1usize)];
    while let Some((current, depth)) = pending.pop() {
        let children: Box<dyn Iterator<Item = &Value>> = match current {
            Value::Array(items) => Box::new(items.iter()),
            Value::Object(map) => Box::new(map.values()),
            _ => continue,
        };
        if depth > MAX_DEPTH {
            return Err(Rejected::TooDeep);
        }
        pending.extend(children.map(|child| (child, depth + 1)));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
