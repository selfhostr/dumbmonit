//! Items SNMP d'un template Zabbix : scalaires, walk (7.0) et règles de
//! découverte (`discovery[…]`, ou `walk[…]` + `SNMP_WALK_TO_JSON`).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::{
    Ctx, Step, is_text_value, item_label, item_type, key_stem, label_of, list, remember, shape,
    steps, text, unit,
};
use crate::import::out::{Draft, numeric_oid, snmp_pattern_ok};

const NEEDS_NUMERIC: &str = "needs numeric OID: MIB names are not resolved, write the OID";

/// Ce que désigne le champ `snmp_oid` d'un item.
#[derive(Debug, PartialEq, Eq)]
enum SnmpOid {
    /// Un scalaire (`….0`).
    Scalar(String),
    /// La colonne d'un prototype (`<colonne>.{#SNMPINDEX}`).
    Column(String),
    /// Une ligne précise d'une table : non lisible par un profil.
    Row(String),
    /// `walk[…]` (7.0) : lu par ses items dépendants.
    Walk,
    /// `discovery[{#M},oid,…]` : macro → OID.
    Discovery(Vec<(String, String)>),
    /// Un nom de MIB, sans OID numérique.
    Named(String),
}

fn bracket<'a>(raw: &'a str, function: &str) -> Option<&'a str> {
    raw.strip_prefix(function)?.strip_prefix('[')?.strip_suffix(']')
}

fn parse_oid(raw: &str) -> SnmpOid {
    let raw = raw.trim();
    if bracket(raw, "walk").is_some() {
        return SnmpOid::Walk;
    }
    if let Some(inner) = bracket(raw, "get") {
        return parse_oid(inner);
    }
    if let Some(inner) = bracket(raw, "discovery") {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        let pairs = parts
            .chunks(2)
            .filter(|pair| pair.len() == 2)
            .map(|pair| (pair[0].to_string(), pair[1].to_string()))
            .collect();
        return SnmpOid::Discovery(pairs);
    }
    if let Some(base) = raw.strip_suffix(".{#SNMPINDEX}") {
        return numeric_oid(base).map_or_else(|| SnmpOid::Named(raw.into()), SnmpOid::Column);
    }
    match numeric_oid(raw) {
        Some(oid) if oid.ends_with(".0") => SnmpOid::Scalar(oid),
        Some(oid) => SnmpOid::Row(oid),
        None => SnmpOid::Named(raw.into()),
    }
}

/// Où lire une métrique.
struct Place {
    oid: String,
    walk: bool,
    labels: Map<String, Value>,
    filters: Option<Value>,
}

/// Un item `SNMP_AGENT` hors découverte.
pub(super) fn item(ctx: &mut Ctx, draft: &mut Draft, profile: &str, item: &Value) {
    let key = text(item, "key");
    match parse_oid(&text(item, "snmp_oid")) {
        SnmpOid::Scalar(oid) => {
            let place = Place { oid, walk: false, labels: Map::new(), filters: None };
            metric(ctx, draft, profile, item, place, &steps(item), "item");
        }
        SnmpOid::Walk => {
            draft.partial("item", &key, "SNMP walk: converted through its dependent items");
        }
        SnmpOid::Discovery(_) => draft.ignored("item", &key, "discovery OID outside a rule"),
        SnmpOid::Row(oid) => draft.ignored(
            "item",
            &key,
            format!(
                "reads one table row ({oid}): profiles read scalars ending in .0 or whole \
                 columns through a discovery rule"
            ),
        ),
        SnmpOid::Column(_) => draft.ignored("item", &key, "{#SNMPINDEX} outside a discovery rule"),
        SnmpOid::Named(name) => draft.ignored("item", &key, format!("{name}: {NEEDS_NUMERIC}")),
    }
}

/// Un item dépendant d'un walk : `SNMP_WALK_VALUE` donne l'OID lu.
pub(super) fn dependent(ctx: &mut Ctx, draft: &mut Draft, profile: &str, item: &Value) {
    let key = text(item, "key");
    let all = steps(item);
    let Some((first, rest)) = all.split_first().filter(|(s, _)| s.kind == "SNMP_WALK_VALUE") else {
        draft.ignored("item", &key, "depends on an SNMP walk without SNMP_WALK_VALUE");
        return;
    };
    match parse_oid(first.params.first().map_or("", String::as_str)) {
        SnmpOid::Scalar(oid) => {
            let place = Place { oid, walk: false, labels: Map::new(), filters: None };
            metric(ctx, draft, profile, item, place, rest, "item");
        }
        SnmpOid::Named(name) => draft.ignored("item", &key, format!("{name}: {NEEDS_NUMERIC}")),
        _ => draft.ignored("item", &key, "reads one table row: only scalars ending in .0"),
    }
}

/// Une règle de découverte SNMP et ses prototypes d'items.
pub(super) fn discovery(
    ctx: &mut Ctx,
    draft: &mut Draft,
    profile: &str,
    rule: &Value,
    by_key: &BTreeMap<String, &Value>,
) {
    let name = text(rule, "name");
    let rule_steps = steps(rule);
    let walk_json = || -> Option<Vec<(String, String)>> {
        let step = rule_steps.iter().find(|s| s.kind == "SNMP_WALK_TO_JSON")?;
        Some(walk_to_json(step))
    };
    let pairs = match item_type(rule).as_str() {
        "SNMP_AGENT" => match parse_oid(&text(rule, "snmp_oid")) {
            SnmpOid::Discovery(pairs) => Some(pairs),
            SnmpOid::Walk => walk_json(),
            _ => None,
        },
        "DEPENDENT" => {
            let master = rule.get("master_item").map(|m| text(m, "key")).unwrap_or_default();
            let snmp_master = by_key.get(&master).is_some_and(|m| item_type(m) == "SNMP_AGENT");
            if snmp_master { walk_json() } else { None }
        }
        _ => None,
    };
    let Some(pairs) = pairs else {
        draft.ignored("discovery", &name, "only SNMP discovery rules are converted");
        return;
    };

    let mut notes = Vec::new();
    let mut labels = Map::new();
    for (macro_name, oid) in &pairs {
        match numeric_oid(oid) {
            Some(oid) => {
                labels.insert(label_of(macro_name), json!(oid));
            }
            None => notes.push(format!("label {macro_name} ({oid}): {NEEDS_NUMERIC}")),
        }
    }
    let filters = filter(ctx, rule, &labels, &mut notes);
    for key in ["overrides", "graph_prototypes", "host_prototypes"] {
        if !list(rule, key).is_empty() {
            notes.push(format!("{key} are not converted"));
        }
    }

    let mut converted = 0;
    for proto in list(rule, "item_prototypes") {
        let key = text(proto, "key");
        let proto_steps = steps(proto);
        let (column, rest): (Option<String>, &[Step]) = match item_type(proto).as_str() {
            "SNMP_AGENT" => match parse_oid(&text(proto, "snmp_oid")) {
                SnmpOid::Column(column) => (Some(column), proto_steps.as_slice()),
                _ => (None, proto_steps.as_slice()),
            },
            "DEPENDENT" => match proto_steps.split_first() {
                Some((first, rest)) if first.kind == "SNMP_WALK_VALUE" => {
                    let raw = first.params.first().map_or("", String::as_str);
                    match parse_oid(raw) {
                        SnmpOid::Column(column) => (Some(column), rest),
                        _ => (None, rest),
                    }
                }
                _ => (None, proto_steps.as_slice()),
            },
            other => {
                let reason = format!("{other} prototypes are not converted");
                draft.ignored("item prototype", &key, reason);
                continue;
            }
        };
        let Some(column) = column else {
            draft.ignored(
                "item prototype",
                &key,
                format!("expects <numeric column OID>.{{#SNMPINDEX}}; {NEEDS_NUMERIC}"),
            );
            continue;
        };
        let place =
            Place { oid: column, walk: true, labels: labels.clone(), filters: filters.clone() };
        if metric(ctx, draft, profile, proto, place, rest, "item prototype") {
            converted += 1;
        }
    }

    if converted == 0 {
        draft.ignored("discovery", &name, "no item prototype could be converted");
    } else if notes.is_empty() {
        draft.converted("discovery", &name, format!("{converted} table metrics"));
    } else {
        draft.partial("discovery", &name, notes.join("; "));
    }
}

/// Paramètres de `SNMP_WALK_TO_JSON` : triplets macro, OID, format.
fn walk_to_json(step: &Step) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut iter = step.params.iter();
    while let Some(param) = iter.next() {
        if param.trim().starts_with("{#")
            && let Some(oid) = iter.next()
        {
            pairs.push((param.trim().to_string(), oid.trim().to_string()));
        }
    }
    pairs
}

/// Filtre d'une découverte : les conditions `NOT_MATCHES_REGEX` deviennent
/// des lignes écartées ; les autres sont rapportées (toutes les lignes restent).
fn filter(
    ctx: &Ctx,
    rule: &Value,
    labels: &Map<String, Value>,
    notes: &mut Vec<String>,
) -> Option<Value> {
    let filter = rule.get("filter")?;
    let conditions = list(filter, "conditions");
    if conditions.is_empty() {
        return None;
    }
    let evaltype = text(filter, "evaltype");
    if matches!(evaltype.as_str(), "OR" | "FORMULA") {
        notes.push(format!("filter ({evaltype}) not converted: every row is kept"));
        return None;
    }
    let mut per_macro: BTreeMap<String, usize> = BTreeMap::new();
    for condition in conditions {
        *per_macro.entry(text(condition, "macro")).or_default() += 1;
    }
    let mut drop: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for condition in conditions {
        let macro_name = text(condition, "macro");
        let operator = text(condition, "operator");
        let operator = if operator.is_empty() { "MATCHES_REGEX".to_string() } else { operator };
        let label = label_of(&macro_name);
        let value = ctx.expand(&text(condition, "value"));
        if value.contains("CHANGE_IF_NEEDED") {
            continue;
        }
        // En AND_OR, deux conditions sur la même macro se combinent par OU :
        // « écarter si l'un correspond » n'en serait pas la traduction.
        let alone = evaltype == "AND" || per_macro.get(&macro_name) == Some(&1);
        if operator != "NOT_MATCHES_REGEX" || !alone || !labels.contains_key(&label) {
            notes.push(format!("filter on {macro_name} ({operator}) not converted: rows are kept"));
        } else if value.starts_with('@') {
            notes.push(format!("filter on {macro_name} uses a global regular expression"));
        } else if snmp_pattern_ok(&value) {
            drop.entry(label).or_default().push(value);
        } else {
            notes.push(format!("filter on {macro_name}: pattern too complex for SNMP profiles"));
        }
    }
    (!drop.is_empty()).then(|| json!({ "drop_when_label_matches": drop }))
}

/// Ajoute la métrique d'un item ou d'un prototype au profil du template.
fn metric(
    ctx: &mut Ctx,
    draft: &mut Draft,
    profile: &str,
    item: &Value,
    place: Place,
    steps: &[Step],
    element: &str,
) -> bool {
    let key = text(item, "key");
    if is_text_value(item) {
        draft.ignored(element, &key, "text value: DumbMonit records numbers");
        return false;
    }
    let shape = match shape(ctx, steps) {
        Ok(shape) => shape,
        Err(reason) => {
            draft.ignored(element, &key, reason);
            return false;
        }
    };
    let mut notes = shape.notes;
    let label = item_label(ctx, item, &mut notes);
    let unit = unit(&text(item, "units"), shape.counter);
    let index = draft.profile(profile);
    let name = draft.metric_name(&key_stem(&key));
    let mut value = json!({
        "name": name,
        "oid": place.oid,
        "kind": if shape.counter { "counter" } else { "gauge" },
        "label": label,
        "view": if place.walk { "timeseries" } else { "stat" },
    });
    if place.walk {
        value["walk"] = json!(true);
        value["view_title"] = json!(label);
        if let Some(first) = place.labels.keys().next() {
            value["entity"] = json!(first);
        }
        if !place.labels.is_empty() {
            value["labels"] = Value::Object(place.labels);
        }
        if let Some(filters) = place.filters {
            value["filters"] = filters;
        }
    }
    if let Some(scale) = shape.scale {
        value["scale"] = json!(scale);
    }
    if !unit.is_empty() {
        value["unit"] = json!(unit);
    }
    if !draft.push_snmp(index, value, element, &key) {
        return false;
    }
    remember(ctx, &key, &name, shape.counter, false, &unit);
    if notes.is_empty() {
        draft.converted(element, &key, format!("SNMP metric {name}"));
    } else {
        draft.partial(element, &key, notes.join("; "));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_formes_doid_sont_reconnues() {
        let uptime = SnmpOid::Scalar("1.3.6.1.2.1.1.3.0".into());
        assert_eq!(parse_oid("1.3.6.1.2.1.1.3.0"), uptime);
        assert_eq!(parse_oid("get[.1.3.6.1.2.1.1.3.0]"), uptime);
        let column = SnmpOid::Column("1.3.6.1.2.1.2.2.1.8".into());
        assert_eq!(parse_oid("1.3.6.1.2.1.2.2.1.8.{#SNMPINDEX}"), column);
        let row = SnmpOid::Row("1.3.6.1.2.1.2.2.1.8.3".into());
        assert_eq!(parse_oid("1.3.6.1.2.1.2.2.1.8.3"), row);
        assert_eq!(parse_oid("walk[1.3.6.1.2.1.2.2.1.8]"), SnmpOid::Walk);
        assert_eq!(
            parse_oid("discovery[{#NAME},1.3.6.1.2.1.2.2.1.2]"),
            SnmpOid::Discovery(vec![("{#NAME}".into(), "1.3.6.1.2.1.2.2.1.2".into())])
        );
        assert!(matches!(parse_oid("SNMPv2-MIB::sysUpTime.0"), SnmpOid::Named(_)));
    }
}
