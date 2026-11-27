//! Import, à l'exécution, de modèles fournis par l'utilisateur : exports de
//! templates Zabbix (YAML 6.0/7.0, ou XML) et fichiers de snmp_exporter
//! (`generator.yml`, ou `snmp.yml` généré, préféré : ses OID sont numériques).
//!
//! Rien n'est embarqué : l'utilisateur apporte son fichier, on le convertit en
//! un paquet d'intégration ordinaire (profils SNMP, sources HTTP, règles), puis
//! le paquet obtenu passe la même vérification que tout paquet
//! ([`crate::Pack::parse`]). Chaque élément du fichier figure au rapport :
//! converti, converti en partie, ou ignoré avec sa raison. Jamais d'échec
//! silencieux.

mod out;
mod snmp_exporter;
mod zabbix;

use serde::Serialize;
use serde_json::Value;

pub use out::{Outcome, ReportEntry};

use out::Draft;

/// Taille maximale d'un fichier importé.
pub const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;

/// Format reconnu du fichier fourni.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat {
    ZabbixYaml,
    ZabbixXml,
    SnmpExporterGenerator,
    SnmpExporterSnmp,
}

/// Réglages choisis par l'utilisateur avant la conversion.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Identifiant du paquet ; à défaut, tiré du nom du modèle.
    pub id: Option<String>,
    /// Libellé du paquet ; à défaut, le nom du modèle.
    pub label: Option<String>,
    /// Préfixes de `sysObjectID` auxquels appliquer les profils SNMP. Vide : le
    /// profil n'est jamais choisi d'office, on le désigne sur l'équipement.
    pub sysobjectid: Vec<String>,
}

/// Ce que la conversion a produit.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub snmp_profiles: Vec<String>,
    pub snmp_metrics: usize,
    pub http_sources: usize,
    pub http_metrics: usize,
    pub rules: Vec<String>,
    pub options: usize,
    pub converted: usize,
    pub partial: usize,
    pub ignored: usize,
}

/// Résultat complet : le paquet en YAML, son résumé, le rapport élément par
/// élément, et les erreurs qui empêchent de l'installer (vide sinon).
#[derive(Debug, Clone, Serialize)]
pub struct Conversion {
    pub format: SourceFormat,
    pub pack_id: String,
    pub label: String,
    pub yaml: String,
    pub summary: Summary,
    pub report: Vec<ReportEntry>,
    pub errors: Vec<String>,
}

impl Conversion {
    /// Vrai si le paquet peut être installé tel quel.
    pub fn is_installable(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Un fichier qu'on ne sait pas lire du tout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportError(pub String);

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImportError {}

/// Reconnaît le format d'un fichier et le convertit en paquet.
pub fn convert(text: &str, options: &ImportOptions) -> Result<Conversion, ImportError> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(ImportError(format!(
            "the file is {} KiB, more than the {} KiB allowed",
            text.len() / 1024,
            MAX_INPUT_BYTES / 1024
        )));
    }
    for prefix in &options.sysobjectid {
        if out::numeric_oid(prefix).is_none() {
            return Err(ImportError(format!("\"{prefix}\" is not a numeric sysObjectID prefix")));
        }
    }
    let (format, document) = detect(text)?;
    let mut draft = Draft::new(options);
    match format {
        SourceFormat::ZabbixYaml | SourceFormat::ZabbixXml => {
            zabbix::convert(&document, &mut draft)?;
        }
        SourceFormat::SnmpExporterGenerator => snmp_exporter::generator(&document, &mut draft)?,
        SourceFormat::SnmpExporterSnmp => snmp_exporter::generated(&document, &mut draft)?,
    }
    Ok(draft.finish(format))
}

/// Le format du fichier, et son contenu lu en arbre JSON.
fn detect(text: &str) -> Result<(SourceFormat, Value), ImportError> {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('<') {
        let document = zabbix::xml_to_value(trimmed)?;
        return Ok((SourceFormat::ZabbixXml, document));
    }
    let document: Value = dumbmonit_collectors::yaml::from_str(trimmed)
        .map_err(|error| ImportError(format!("the file is neither YAML nor XML: {error}")))?;
    let Some(root) = document.as_object() else {
        return Err(unknown());
    };
    if root.contains_key("zabbix_export") {
        return Ok((SourceFormat::ZabbixYaml, document));
    }
    // snmp_exporter récent : `auths` + `modules` ; ancien : les modules à la racine.
    let modules = match root.get("modules").and_then(Value::as_object) {
        Some(modules) => modules,
        None => root,
    };
    let looks_generated = modules.values().any(|module| module.get("metrics").is_some());
    let looks_generator = modules.values().any(|module| {
        module.get("walk").is_some() && module.get("metrics").is_none()
    });
    if looks_generated {
        Ok((SourceFormat::SnmpExporterSnmp, document))
    } else if looks_generator {
        Ok((SourceFormat::SnmpExporterGenerator, document))
    } else {
        Err(unknown())
    }
}

fn unknown() -> ImportError {
    ImportError(
        "unrecognised file: expected a Zabbix template export (YAML or XML), or a \
         snmp_exporter generator.yml or snmp.yml"
            .into(),
    )
}
