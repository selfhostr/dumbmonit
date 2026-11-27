//! snmp_exporter : `snmp.yml` (généré, OID numériques, préféré) et
//! `generator.yml` (sa source, dont les noms ne se résolvent qu'avec les MIB).
//!
//! Un module devient un profil SNMP. Aucune MIB n'est lue : un nom d'objet
//! (`ifHCInOctets`) sans son OID numérique est rapporté tel quel, avec le
//! conseil d'importer plutôt le `snmp.yml` généré.

use serde_json::{Map, Value, json};

use super::ImportError;
use super::out::{Draft, camel_to_snake, numeric_oid, snake, with_states};

const NEEDS_NUMERIC: &str = "needs numeric OID: names are resolved from MIB files, which \
                             DumbMonit does not read; import the generated snmp.yml instead, \
                             or write the OID";

/// Types de valeur numériques de snmp_exporter, et leur nature chez nous.
fn numeric_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "counter" => Some("counter"),
        "gauge" | "Float" | "Double" | "EnumAsInfo" | "EnumAsStateSet" | "Integer32" => {
            Some("gauge")
        }
        _ => None,
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn modules(doc: &Value) -> Map<String, Value> {
    let root = doc.as_object().cloned().unwrap_or_default();
    match root.get("modules").and_then(Value::as_object) {
        Some(modules) => modules.clone(),
        // Ancien `snmp.yml` : les modules à la racine, sans `auths`.
        None => root.into_iter().filter(|(_, module)| module.is_object()).collect(),
    }
}

fn identify(draft: &mut Draft, modules: &Map<String, Value>, file: &str) {
    let name = match modules.keys().next() {
        Some(only) if modules.len() == 1 => only.clone(),
        _ => "snmp-exporter".to_string(),
    };
    draft.identify(&name, &format!("Imported from a snmp_exporter {file}."));
}

fn report_auths(doc: &Value, draft: &mut Draft) {
    if doc.get("auths").is_some() {
        draft.ignored("auths", "auths", "SNMP credentials are set on each device in DumbMonit");
    }
}

// ---------------------------------------------------------------------------
// snmp.yml
// ---------------------------------------------------------------------------

pub(super) fn generated(doc: &Value, draft: &mut Draft) -> Result<(), ImportError> {
    let modules = modules(doc);
    if modules.is_empty() {
        return Err(ImportError("this snmp.yml declares no module".into()));
    }
    identify(draft, &modules, "snmp.yml");
    report_auths(doc, draft);
    for (module, body) in &modules {
        let Some(metrics) = body.get("metrics").and_then(Value::as_array) else {
            draft.ignored("module", module, "no metrics in this module");
            continue;
        };
        let profile = draft.profile(module);
        for metric in metrics {
            generated_metric(draft, profile, module, metric);
        }
    }
    Ok(())
}

fn generated_metric(draft: &mut Draft, profile: usize, module: &str, metric: &Value) {
    let name = text(metric, "name");
    let element = format!("{module}.{name}");
    let Some(oid) = numeric_oid(text(metric, "oid")) else {
        draft.ignored("metric", &element, "no numeric OID");
        return;
    };
    let kind_name = text(metric, "type");
    let Some(kind) = numeric_kind(kind_name) else {
        draft.ignored(
            "metric",
            &element,
            format!(
                "{kind_name} values are text: DumbMonit records numbers (a text column can \
                 still label other metrics through a lookup)"
            ),
        );
        return;
    };
    if metric.get("regex_extracts").is_some() {
        draft.ignored("metric", &element, "regex_extracts are not supported");
        return;
    }

    let mut notes = Vec::new();
    let indexes: Vec<&str> = metric
        .get("indexes")
        .and_then(Value::as_array)
        .map(|list| list.iter().map(|index| text(index, "labelname")).collect())
        .unwrap_or_default();
    let walk = !indexes.is_empty();
    if walk && oid.ends_with(".0") {
        draft.ignored("metric", &element, "a table column cannot end in .0");
        return;
    }
    let instance = if walk { oid.clone() } else { format!("{oid}.0") };
    if indexes.len() > 1 {
        notes.push("the multi-part index is kept whole in the \"index\" label".to_string());
    }

    let mut labels = Map::new();
    for lookup in metric.get("lookups").and_then(Value::as_array).into_iter().flatten() {
        let labelname = text(lookup, "labelname");
        let keys: Vec<&str> = lookup
            .get("labels")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        match numeric_oid(text(lookup, "oid")) {
            Some(column) if keys == indexes => {
                labels.insert(label_name(labelname), json!(column));
            }
            Some(_) => notes.push(format!("lookup {labelname} uses another index: not kept")),
            None => notes.push(format!("lookup {labelname} has no numeric OID: not kept")),
        }
    }

    let states = states(metric.get("enum_values"));
    let mut value = json!({
        "name": draft.metric_name(&camel_to_snake(name)),
        "oid": instance,
        "kind": kind,
        "label": with_states(name, &states),
        "view": if walk { "timeseries" } else { "stat" },
    });
    if walk {
        value["walk"] = json!(true);
        value["view_title"] = json!(name);
        if let Some(first) = labels.keys().next() {
            value["entity"] = json!(first);
        }
        if !labels.is_empty() {
            value["labels"] = Value::Object(labels);
        }
    }
    if let Some(scale) = metric.get("scale").and_then(Value::as_f64) {
        value["scale"] = json!(scale);
    }
    if metric.get("offset").is_some() {
        notes.push("offset is not supported: raw value kept".into());
    }
    if !states.is_empty() {
        notes.push("enum values are shown in the label; the series keeps the code".into());
    }
    if draft.push_snmp(profile, value, "metric", &element) {
        finish(draft, "metric", &element, notes);
    }
}

fn label_name(labelname: &str) -> String {
    let name = camel_to_snake(labelname);
    if name == "index" { "index_label".into() } else { name }
}

fn states(values: Option<&Value>) -> Vec<(String, String)> {
    let Some(map) = values.and_then(Value::as_object) else { return Vec::new() };
    let mut states: Vec<(String, String)> = map
        .iter()
        .map(|(code, text)| {
            (code.clone(), text.as_str().map_or_else(|| text.to_string(), str::to_string))
        })
        .collect();
    states.sort_by_key(|(code, _)| code.parse::<i64>().unwrap_or(i64::MAX));
    states
}

fn finish(draft: &mut Draft, kind: &str, element: &str, notes: Vec<String>) {
    if notes.is_empty() {
        draft.converted(kind, element, "converted");
    } else {
        draft.partial(kind, element, notes.join("; "));
    }
}

// ---------------------------------------------------------------------------
// generator.yml
// ---------------------------------------------------------------------------

pub(super) fn generator(doc: &Value, draft: &mut Draft) -> Result<(), ImportError> {
    let modules = modules(doc);
    if modules.is_empty() {
        return Err(ImportError("this generator.yml declares no module".into()));
    }
    identify(draft, &modules, "generator.yml");
    report_auths(doc, draft);
    for (module, body) in &modules {
        let profile = draft.profile(module);
        let overrides = body.get("overrides").and_then(Value::as_object).cloned();
        let overrides = overrides.unwrap_or_default();

        // Étiquettes des lookups à OID numérique, supposés porter l'index des colonnes.
        let mut labels = Map::new();
        for lookup in body.get("lookups").and_then(Value::as_array).into_iter().flatten() {
            let target = text(lookup, "lookup");
            match numeric_oid(target) {
                Some(oid) => {
                    let name = format!("lookup_{}", labels.len() + 1);
                    draft.partial(
                        "lookup",
                        target,
                        format!("kept as the label \"{name}\" of every column of this module"),
                    );
                    labels.insert(name, json!(oid));
                }
                None => draft.ignored("lookup", target, NEEDS_NUMERIC),
            }
        }

        let list = |key: &str| -> Vec<String> {
            body.get(key)
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default()
        };
        let gets = list("get");
        for entry in list("walk").iter().chain(gets.iter()) {
            let from_get = gets.contains(entry);
            generator_entry(draft, profile, module, entry, from_get, &overrides, &labels);
        }
        for key in ["filters", "max_repetitions", "retries", "timeout"] {
            if body.get(key).is_some() {
                draft.ignored("setting", &format!("{module}.{key}"), "not used by DumbMonit");
            }
        }
    }
    Ok(())
}

fn generator_entry(
    draft: &mut Draft,
    profile: usize,
    module: &str,
    entry: &str,
    from_get: bool,
    overrides: &Map<String, Value>,
    labels: &Map<String, Value>,
) {
    let element = format!("{module}.{entry}");
    let Some(oid) = numeric_oid(entry) else {
        draft.ignored("walk", &element, NEEDS_NUMERIC);
        return;
    };
    let rule = overrides.get(entry).cloned().unwrap_or(Value::Null);
    if rule.get("ignore").and_then(Value::as_bool) == Some(true) {
        draft.ignored("walk", &element, "ignored by an override");
        return;
    }
    let declared = text(&rule, "type");
    let kind = if declared.is_empty() { Some("gauge") } else { numeric_kind(declared) };
    let Some(kind) = kind else {
        draft.ignored("walk", &element, format!("{declared} values are text: not recorded"));
        return;
    };
    let scalar = oid.ends_with(".0");
    if from_get && !scalar {
        draft.ignored("get", &element, "reads one table row: only scalars (.0) or columns");
        return;
    }
    let mut notes = Vec::new();
    if declared.is_empty() {
        notes.push("type unknown without MIB: recorded as a gauge".to_string());
    }
    let stem = oid.strip_prefix("1.3.6.1.").unwrap_or(&oid).replace('.', "_");
    let mut value = json!({
        "name": draft.metric_name(&format!("oid_{}", snake(&stem))),
        "oid": oid,
        "kind": kind,
        "label": entry,
        "view": if scalar { "stat" } else { "timeseries" },
    });
    if !scalar {
        notes.push(
            "read as one table column: list the columns of a table or subtree one by one"
                .to_string(),
        );
        value["walk"] = json!(true);
        value["view_title"] = json!(entry);
        if !labels.is_empty() {
            value["labels"] = Value::Object(labels.clone());
        }
    }
    if rule.get("regex_extracts").is_some() {
        notes.push("regex_extracts are not supported".into());
    }
    if draft.push_snmp(profile, value, "walk", &element) {
        finish(draft, "walk", &element, notes);
    }
}
