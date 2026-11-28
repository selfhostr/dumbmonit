//! Ce que les convertisseurs remplissent : le paquet en cours (profils SNMP,
//! sources HTTP, règles), le rapport, les noms uniques ; puis l'écriture du
//! YAML et sa vérification par [`crate::Pack::parse`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::{Conversion, ImportOptions, SourceFormat, Summary};
use crate::{MAX_METRICS, MAX_RULES, Pack, prefix_for};

/// Entrées de rapport conservées au plus : au-delà, une seule ligne le dit.
const MAX_REPORT: usize = 2_000;

/// Sort d'un élément du fichier importé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Converted,
    Partial,
    Ignored,
}

/// Une ligne du rapport : quel élément, ce qu'il est devenu, pourquoi.
#[derive(Debug, Clone, Serialize)]
pub struct ReportEntry {
    /// Nature de l'élément : `item`, `discovery`, `trigger`, `module`, `metric`…
    pub element: String,
    /// Son nom ou sa clé dans le fichier.
    pub name: String,
    pub outcome: Outcome,
    /// En anglais, comme les autres textes du serveur.
    pub reason: String,
}

/// Un profil SNMP en cours de construction (un par template ou module).
#[derive(Debug, Default)]
pub(crate) struct ProfileDraft {
    pub id: String,
    pub name: String,
    pub metrics: Vec<Value>,
}

/// La partie HTTP du paquet.
#[derive(Debug, Default)]
pub(crate) struct HttpDraft {
    pub sources: Vec<Value>,
    pub metrics: Vec<Value>,
    pub options: Vec<Value>,
    /// Macro (`{$X}`) → clé d'option déjà déclarée.
    pub option_keys: BTreeMap<String, String>,
    pub credential: Option<&'static str>,
    pub scheme: Option<&'static str>,
    pub default_port: Option<u16>,
}

/// Une métrique convertie, pour qu'un déclencheur puisse la citer.
#[derive(Debug, Clone)]
pub(crate) struct MetricRef {
    /// Nom complet de la série (`dumbmonit_<préfixe><nom>`).
    pub full: String,
    pub counter: bool,
    pub http: bool,
    pub unit: String,
}

pub(crate) struct Draft {
    options: ImportOptions,
    pub id: String,
    pub label: String,
    pub summary_text: String,
    pub profiles: Vec<ProfileDraft>,
    pub http: HttpDraft,
    pub rules: Vec<Value>,
    names: BTreeSet<String>,
    source_ids: BTreeSet<String>,
    rule_names: BTreeSet<String>,
    report: Vec<ReportEntry>,
    dropped: usize,
}

impl Draft {
    pub fn new(options: &ImportOptions) -> Self {
        Self {
            options: options.clone(),
            id: String::new(),
            label: String::new(),
            summary_text: String::new(),
            profiles: Vec::new(),
            http: HttpDraft::default(),
            rules: Vec::new(),
            names: BTreeSet::new(),
            source_ids: BTreeSet::new(),
            rule_names: BTreeSet::new(),
            report: Vec::new(),
            dropped: 0,
        }
    }

    /// Fixe l'identifiant et le libellé d'après le nom du modèle, sauf choix
    /// de l'utilisateur. Un identifiant déjà pris par un profil livré reçoit
    /// le préfixe `custom-`, pour que le paquet par défaut s'installe.
    pub fn identify(&mut self, name: &str, summary: &str) {
        if !self.id.is_empty() {
            return;
        }
        self.label = match self.options.label.as_deref().map(str::trim) {
            Some(label) if !label.is_empty() => label.to_string(),
            _ => clean_text(name),
        };
        if self.label.is_empty() {
            self.label = "Imported template".into();
        }
        self.id = match self.options.id.as_deref().map(str::trim) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => {
                let slug = slug_id(name);
                let catalog = dumbmonit_collectors::SnmpCollector::new();
                let taken = |id: &str| {
                    let family = format!("{id}-");
                    catalog
                        .catalog()
                        .profiles()
                        .iter()
                        .any(|p| p.id == id || p.id.starts_with(&family))
                };
                if taken(&slug) { slug_id(&format!("custom-{slug}")) } else { slug }
            }
        };
        self.summary_text = clean_text(summary).chars().take(240).collect();
    }

    pub fn prefix(&self) -> String {
        prefix_for(&self.id)
    }

    /// Un nom de métrique unique dans le paquet, préfixe compris.
    pub fn metric_name(&mut self, base: &str) -> String {
        let stem: String = snake(base).chars().take(64).collect();
        let wanted = format!("{}{}", self.prefix(), stem.trim_end_matches('_'));
        unique(&mut self.names, &wanted)
    }

    pub fn source_id(&mut self, base: &str) -> String {
        let stem: String = snake(base).chars().take(40).collect();
        unique(&mut self.source_ids, stem.trim_end_matches('_'))
    }

    /// L'indice du profil d'un template, créé au besoin. Le premier porte
    /// l'identifiant du paquet, les suivants `<paquet>-<template>`.
    pub fn profile(&mut self, name: &str) -> usize {
        if let Some(index) = self.profiles.iter().position(|p| p.name == name) {
            return index;
        }
        let id = if self.profiles.is_empty() {
            self.id.clone()
        } else {
            let mut id = format!("{}-{}", self.id, slug_id(name));
            id.truncate(80);
            let base = id.trim_end_matches('-').to_string();
            let mut candidate = base.clone();
            let mut n = 2;
            while self.profiles.iter().any(|p| p.id == candidate) {
                candidate = format!("{base}-{n}");
                n += 1;
            }
            candidate
        };
        self.profiles.push(ProfileDraft { id, name: name.to_string(), metrics: Vec::new() });
        self.profiles.len() - 1
    }

    /// Ajoute une métrique SNMP ; faux (et rapporté) au-delà du plafond.
    pub fn push_snmp(&mut self, profile: usize, metric: Value, element: &str, name: &str) -> bool {
        if self.profiles[profile].metrics.len() >= MAX_METRICS {
            self.ignored(element, name, format!("more than {MAX_METRICS} metrics in one profile"));
            return false;
        }
        self.profiles[profile].metrics.push(metric);
        true
    }

    pub fn push_http_metric(&mut self, metric: Value, element: &str, name: &str) -> bool {
        if self.http.metrics.len() >= MAX_METRICS {
            self.ignored(element, name, format!("more than {MAX_METRICS} metrics in one pack"));
            return false;
        }
        self.http.metrics.push(metric);
        true
    }

    /// Ajoute une règle d'alerte sous un nom unique ; faux au-delà du plafond.
    pub fn push_rule(&mut self, base: &str, mut rule: Map<String, Value>, element: &str) -> bool {
        if self.rules.len() >= MAX_RULES {
            self.ignored(element, base, format!("more than {MAX_RULES} rules in one pack"));
            return false;
        }
        let stem: String = snake(base).chars().take(50).collect();
        let name = unique(&mut self.rule_names, stem.trim_end_matches('_'));
        rule.insert("name".into(), json!(name));
        self.rules.push(Value::Object(rule));
        true
    }

    /// Option d'un paquet HTTP pour une macro `{$X}` ; déclarée au premier usage.
    pub fn option_for(&mut self, macro_name: &str, default: &str) -> String {
        if let Some(key) = self.http.option_keys.get(macro_name) {
            return key.clone();
        }
        let inner = macro_name.trim_start_matches("{$").trim_end_matches('}');
        let mut stem: String = snake(inner).chars().take(40).collect();
        // Réglages de connexion que tout paquet reçoit : clés réservées.
        const RESERVED: &[&str] =
            &["scheme", "port", "insecure_tls", "request_timeout_seconds", "allow_private_targets"];
        if RESERVED.contains(&stem.as_str()) {
            stem = format!("template_{stem}");
        }
        let mut keys: BTreeSet<String> = self.http.option_keys.values().cloned().collect();
        let key = unique(&mut keys, stem.trim_end_matches('_'));
        let numeric = default.trim().parse::<f64>().is_ok();
        self.http.options.push(json!({
            "key": key,
            "label": inner,
            "help": format!("Imported from the template macro {macro_name}."),
            "default": default,
            "input": if numeric { "number" } else { "text" },
        }));
        self.http.option_keys.insert(macro_name.to_string(), key.clone());
        key
    }

    pub fn entry(&mut self, element: &str, name: &str, outcome: Outcome, reason: String) {
        if self.report.len() >= MAX_REPORT {
            self.dropped += 1;
            return;
        }
        self.report.push(ReportEntry {
            element: element.to_string(),
            name: name.chars().take(200).collect(),
            outcome,
            reason,
        });
    }

    pub fn converted(&mut self, element: &str, name: &str, reason: impl Into<String>) {
        self.entry(element, name, Outcome::Converted, reason.into());
    }

    pub fn partial(&mut self, element: &str, name: &str, reason: impl Into<String>) {
        self.entry(element, name, Outcome::Partial, reason.into());
    }

    pub fn ignored(&mut self, element: &str, name: &str, reason: impl Into<String>) {
        self.entry(element, name, Outcome::Ignored, reason.into());
    }

    /// Écrit le paquet, le vérifie et rend la conversion complète.
    pub fn finish(mut self, format: SourceFormat) -> Conversion {
        if self.id.is_empty() {
            self.identify("imported", "");
        }
        if self.dropped > 0 {
            let dropped = self.dropped;
            self.dropped = 0;
            self.report.push(ReportEntry {
                element: "report".into(),
                name: String::new(),
                outcome: Outcome::Ignored,
                reason: format!("{dropped} more entries are not listed"),
            });
        }
        let sysobjectid: Vec<String> =
            self.options.sysobjectid.iter().filter_map(|oid| numeric_oid(oid)).collect();
        let profiles: Vec<Value> = self
            .profiles
            .iter()
            .filter(|profile| !profile.metrics.is_empty())
            .map(|profile| {
                let mut value = json!({
                    "id": profile.id,
                    "name": clean_text(&profile.name),
                    "include": ["system"],
                    "metrics": profile.metrics,
                });
                if !sysobjectid.is_empty() {
                    value["match"] = json!({ "sysobjectid": sysobjectid });
                }
                value
            })
            .collect();

        let mut doc: Vec<(&str, Value)> = vec![
            ("schema", json!(1)),
            ("id", json!(self.id)),
            ("version", json!("1.0.0")),
            ("label", json!(self.label)),
        ];
        if !self.summary_text.is_empty() {
            doc.push(("summary", json!(self.summary_text)));
        }
        let http = !self.http.sources.is_empty();
        if http {
            if let Some(scheme) = self.http.scheme {
                doc.push(("scheme", json!(scheme)));
            }
            if let Some(port) = self.http.default_port {
                doc.push(("default_port", json!(port)));
            }
            doc.push(("credentials", json!([self.http.credential.unwrap_or("none")])));
            if !self.http.options.is_empty() {
                doc.push(("options", Value::Array(std::mem::take(&mut self.http.options))));
            }
            doc.push(("sources", Value::Array(std::mem::take(&mut self.http.sources))));
            doc.push(("metrics", Value::Array(std::mem::take(&mut self.http.metrics))));
        }
        if !self.rules.is_empty() {
            doc.push(("rules", Value::Array(self.rules.clone())));
        }
        if !profiles.is_empty() {
            doc.push(("snmp_profiles", Value::Array(profiles.clone())));
        }
        let yaml = document(&doc);

        let mut summary = Summary {
            snmp_profiles: profiles
                .iter()
                .filter_map(|p| p["id"].as_str().map(str::to_string))
                .collect(),
            snmp_metrics: profiles.iter().map(|p| len(&p["metrics"])).sum(),
            http_sources: doc.iter().find(|(k, _)| *k == "sources").map_or(0, |(_, v)| len(v)),
            http_metrics: doc.iter().find(|(k, _)| *k == "metrics").map_or(0, |(_, v)| len(v)),
            rules: self
                .rules
                .iter()
                .filter_map(|r| r["name"].as_str().map(|n| format!("pack:{}:{n}", self.id)))
                .collect(),
            options: doc.iter().find(|(k, _)| *k == "options").map_or(0, |(_, v)| len(v)),
            ..Summary::default()
        };
        for entry in &self.report {
            match entry.outcome {
                Outcome::Converted => summary.converted += 1,
                Outcome::Partial => summary.partial += 1,
                Outcome::Ignored => summary.ignored += 1,
            }
        }

        let errors = if profiles.is_empty() && !http {
            vec!["nothing in this file could be converted: see the report".to_string()]
        } else {
            match Pack::parse(&yaml) {
                Ok(_) => Vec::new(),
                Err(error) => error.errors,
            }
        };
        Conversion {
            format,
            pack_id: self.id,
            label: self.label,
            yaml,
            summary,
            report: self.report,
            errors,
        }
    }
}

fn len(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn unique(taken: &mut BTreeSet<String>, base: &str) -> String {
    let base = if base.is_empty() { "value" } else { base };
    let mut candidate = base.to_string();
    let mut n = 2;
    while taken.contains(&candidate) {
        candidate = format!("{base}_{n}");
        n += 1;
    }
    taken.insert(candidate.clone());
    candidate
}

// ---------------------------------------------------------------------------
// Noms et textes
// ---------------------------------------------------------------------------

/// `Battery capacity`, `net.if.in[eth0]` → `battery_capacity`, `net_if_in_eth0`.
pub(crate) fn snake(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    match out.chars().next() {
        None => "value".into(),
        Some(c) if c.is_ascii_digit() => format!("m_{out}"),
        Some(_) => out,
    }
}

/// `ifHCInOctets` → `if_hc_in_octets`.
pub(crate) fn camel_to_snake(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
            if prev.is_ascii_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_ascii_uppercase() && next_lower)
            {
                out.push('_');
            }
        }
        out.push(c);
    }
    snake(&out)
}

/// Identifiant de paquet : minuscules, chiffres et tirets simples, 40 au plus.
pub(crate) fn slug_id(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out = out.trim_matches('-').to_string();
    if out.is_empty() {
        out = "imported".into();
    }
    if !out.starts_with(|c: char| c.is_ascii_lowercase()) {
        out = format!("t-{out}");
    }
    out.truncate(40);
    out.trim_end_matches('-').to_string()
}

static MACRO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{[^{}]*\}").expect("expression valide"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("expression valide"));

/// Un nom affichable : macros retirées, espaces resserrés, sans ponctuation
/// orpheline (`Interface {#IFNAME}: Bits` → `Interface: Bits`).
pub(crate) fn clean_text(text: &str) -> String {
    let without = MACRO.replace_all(text, "");
    let spaced = SPACES.replace_all(&without, " ");
    let tidy = spaced.replace(" :", ":").replace("()", "").replace("[]", "");
    tidy.trim_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '-' | ',' | '/'))
        .chars()
        .take(120)
        .collect()
}

/// Un OID numérique (`.1.3.6.1.2.1.1.3.0` → `1.3.6.1.2.1.1.3.0`), ou `None`.
pub(crate) fn numeric_oid(text: &str) -> Option<String> {
    let text = text.trim().trim_start_matches('.');
    let arcs: Vec<&str> = text.split('.').collect();
    if arcs.len() < 2 || arcs.iter().any(|arc| arc.is_empty() || arc.parse::<u64>().is_err()) {
        return None;
    }
    Some(arcs.join("."))
}

/// Vrai si le motif est accepté par les filtres des profils SNMP.
pub(crate) fn snmp_pattern_ok(pattern: &str) -> bool {
    dumbmonit_collectors::snmp::Pattern::compile(pattern).is_ok()
}

/// Libellé suivi d'une table de valeurs courte : `Status (1 = up, 2 = down)`.
pub(crate) fn with_states(label: &str, states: &[(String, String)]) -> String {
    if states.is_empty() {
        return label.to_string();
    }
    let shown: Vec<String> =
        states.iter().take(6).map(|(value, text)| format!("{value} = {text}")).collect();
    let more = if states.len() > 6 { ", …" } else { "" };
    format!("{label} ({}{more})", shown.join(", "))
}

// ---------------------------------------------------------------------------
// Écriture YAML
// ---------------------------------------------------------------------------

/// Écrit un document YAML lisible, dans l'ordre des clés de tête donné. Les
/// chaînes sont toujours entre guillemets doubles, échappées comme en JSON
/// (forme valide en YAML) : aucune valeur ne peut être relue autrement.
pub(crate) fn document(entries: &[(&str, Value)]) -> String {
    let mut out = String::from("# Converted by DumbMonit from an imported template.\n");
    for (key, value) in entries {
        entry(&mut out, 0, true, key, value);
    }
    out
}

fn pad(out: &mut String, indent: usize) {
    out.extend(std::iter::repeat_n(' ', indent));
}

fn key_text(key: &str) -> String {
    static PLAIN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_.-]*$").expect("expression valide"));
    let reserved = matches!(key, "true" | "false" | "null" | "True" | "False" | "Null" | "NULL");
    if PLAIN.is_match(key) && !reserved { key.to_string() } else { scalar(&json!(key)) }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::Object(_) => "{}".into(),
        Value::Array(_) => "[]".into(),
        other => other.to_string(),
    }
}

fn entry(out: &mut String, indent: usize, padded: bool, key: &str, value: &Value) {
    if padded {
        pad(out, indent);
    }
    out.push_str(&key_text(key));
    out.push(':');
    match value {
        Value::Object(map) if !map.is_empty() => {
            out.push('\n');
            for (k, v) in map {
                entry(out, indent + 2, true, k, v);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            out.push('\n');
            for item in items {
                list_item(out, indent + 2, item);
            }
        }
        other => {
            out.push(' ');
            out.push_str(&scalar(other));
            out.push('\n');
        }
    }
}

fn list_item(out: &mut String, indent: usize, value: &Value) {
    pad(out, indent);
    out.push_str("- ");
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (index, (k, v)) in map.iter().enumerate() {
                entry(out, indent + 2, index > 0, k, v);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            out.push('\n');
            for item in items {
                list_item(out, indent + 2, item);
            }
        }
        other => {
            out.push_str(&scalar(other));
            out.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_yaml_ecrit_se_relit_a_lidentique() {
        let value = json!({
            "a": "x: \"quoted\"\nline",
            "list": [{"k": 1, "m": {"1": 0.5, "true": false}}, [], {}, "s", ["n"]],
            "empty": {},
            "n": null,
        });
        let text = document(&[("root", value.clone())]);
        let back: Value = dumbmonit_collectors::yaml::from_str(&text).unwrap();
        assert_eq!(back["root"], value, "{text}");
    }

    #[test]
    fn les_noms_sont_nettoyes() {
        assert_eq!(camel_to_snake("ifHCInOctets"), "if_hc_in_octets");
        assert_eq!(camel_to_snake("sysUpTime"), "sys_up_time");
        assert_eq!(snake("net.if.in[{#SNMPINDEX}]"), "net_if_in_snmpindex");
        assert_eq!(snake("1st"), "m_1st");
        assert_eq!(slug_id("Acme UPS by SNMP!"), "acme-ups-by-snmp");
        assert_eq!(slug_id("42 things"), "t-42-things");
        assert_eq!(clean_text("Interface {#IFNAME}: Bits received"), "Interface: Bits received");
        assert_eq!(numeric_oid(".1.3.6.1.2.1.1.3.0").as_deref(), Some("1.3.6.1.2.1.1.3.0"));
        assert_eq!(numeric_oid("IF-MIB::ifDescr"), None);
    }
}
