//! Exports de templates Zabbix (`zabbix_export`, YAML 6.0/7.0, ou XML).
//!
//! Seuls les items SNMP (scalaires, walk, découverte) et HTTP agent (avec
//! JSONPath ou expression régulière) deviennent des mesures ; les macros
//! `{$X}` donnent leur valeur par défaut (ou une option du paquet HTTP), les
//! tables de valeurs se lisent dans le libellé, les déclencheurs simples
//! deviennent des règles. Le reste est listé au rapport avec sa raison.

mod http;
mod snmp;
mod triggers;

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::ImportError;
use super::out::{Draft, MetricRef, clean_text, snake, with_states};

/// État partagé d'une conversion.
#[derive(Debug, Default)]
pub(super) struct Ctx {
    /// `{$X}` → valeur par défaut.
    macros: BTreeMap<String, String>,
    /// Macros déclarées secrètes (type `SECRET_TEXT` ou `VAULT`).
    secret: Vec<String>,
    /// Nom d'une table de valeurs → (valeur, texte).
    valuemaps: BTreeMap<String, Vec<(String, String)>>,
    /// Clé d'item → métrique produite, pour les déclencheurs.
    metrics: BTreeMap<String, MetricRef>,
    /// Clé d'un item HTTP maître → (source, format).
    sources: BTreeMap<String, (String, String)>,
}

impl Ctx {
    /// Valeur par défaut d'une macro (`{$X}` ou `{$X:"contexte"}`).
    fn resolve(&self, name: &str) -> Option<&str> {
        self.macros.get(&base_macro(name)).map(String::as_str)
    }

    fn is_secret(&self, name: &str) -> bool {
        let base = base_macro(name);
        let upper = base.to_ascii_uppercase();
        self.secret.contains(&base)
            || ["PASSWORD", "PASSWD", "TOKEN", "SECRET", "APIKEY", "API_KEY", "API.KEY"]
                .iter()
                .any(|word| upper.contains(word))
    }

    /// Remplace chaque macro utilisateur connue par sa valeur par défaut.
    fn expand(&self, text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find("{$") {
            out.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}') else {
                out.push_str(&rest[start..]);
                return out;
            };
            let token = &rest[start..start + end + 1];
            out.push_str(self.resolve(token).unwrap_or(token));
            rest = &rest[start + end + 1..];
        }
        out.push_str(rest);
        out
    }
}

/// `{$X:"ctx"}` → `{$X}`.
fn base_macro(name: &str) -> String {
    let inner = name.trim().trim_start_matches("{$").trim_end_matches('}');
    let inner = inner.split(':').next().unwrap_or(inner);
    format!("{{${inner}}}")
}

/// Une valeur lue comme texte, qu'elle soit chaîne, nombre ou booléen.
fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

/// Type d'un item : absent de l'export, c'est un item d'agent passif.
fn item_type(item: &Value) -> String {
    let kind = text(item, "type");
    if kind.is_empty() { "ZABBIX_PASSIVE".into() } else { kind }
}

fn is_text_value(item: &Value) -> bool {
    matches!(text(item, "value_type").as_str(), "CHAR" | "TEXT" | "LOG" | "BINARY")
}

/// Une étape de prétraitement.
#[derive(Debug, Clone)]
struct Step {
    kind: String,
    params: Vec<String>,
}

fn steps(item: &Value) -> Vec<Step> {
    list(item, "preprocessing")
        .iter()
        .map(|step| {
            let params = match step.get("parameters") {
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|p| p.as_str().map_or_else(|| p.to_string(), str::to_string))
                    .collect(),
                // 5.x : un seul champ `params`, valeurs séparées par des sauts de ligne.
                _ => text(step, "params").split('\n').map(str::to_string).collect(),
            };
            Step { kind: text(step, "type"), params }
        })
        .collect()
}

/// Ce que les étapes numériques font de la valeur.
#[derive(Debug, Default)]
struct Shape {
    counter: bool,
    scale: Option<f64>,
    notes: Vec<String>,
}

fn shape(ctx: &Ctx, steps: &[Step]) -> Result<Shape, String> {
    let mut shape = Shape::default();
    for step in steps {
        match step.kind.as_str() {
            "MULTIPLIER" => {
                let raw = ctx.expand(step.params.first().map_or("", String::as_str));
                let factor: f64 = raw
                    .trim()
                    .parse()
                    .map_err(|_| format!("multiplier \"{raw}\" is not a number"))?;
                shape.scale = Some(shape.scale.unwrap_or(1.0) * factor);
            }
            "CHANGE_PER_SECOND" | "SIMPLE_CHANGE" => {
                shape.counter = true;
                if step.kind == "SIMPLE_CHANGE" {
                    let note = "SIMPLE_CHANGE recorded as a counter (per-second rate)";
                    shape.notes.push(note.to_string());
                }
            }
            "DISCARD_UNCHANGED" | "DISCARD_UNCHANGED_HEARTBEAT" | "CHECK_NOT_SUPPORTED" => {
                shape.notes.push(format!("{} step dropped", step.kind));
            }
            other => return Err(format!("preprocessing step {other} is not supported")),
        }
    }
    Ok(shape)
}

/// Unité DumbMonit d'une unité Zabbix ; pour un compteur, celle du cumul.
fn unit(units: &str, counter: bool) -> String {
    let units = units.trim().trim_start_matches('!');
    let mapped = match units {
        "uptime" | "unixtime" => "s",
        other => other,
    };
    let mapped = if counter {
        mapped.strip_suffix("ps").or_else(|| mapped.strip_suffix("/s")).unwrap_or(mapped)
    } else {
        mapped
    };
    mapped.chars().take(16).collect()
}

/// Libellé d'un item, suivi de sa table de valeurs s'il en a une.
fn item_label(ctx: &Ctx, item: &Value, notes: &mut Vec<String>) -> String {
    let label = clean_text(&text(item, "name"));
    let map = item.get("valuemap").map(|v| text(v, "name")).unwrap_or_default();
    match ctx.valuemaps.get(&map) {
        Some(states) if !states.is_empty() => {
            let note = format!("value map \"{map}\" shown in the label, the series keeps the code");
            notes.push(note);
            with_states(&label, states)
        }
        _ => label,
    }
}

/// Racine d'un nom de métrique tirée de la clé d'item.
fn key_stem(key: &str) -> String {
    snake(&clean_text(key))
}

fn unsupported(kind: &str) -> String {
    match kind {
        "ZABBIX_PASSIVE" | "ZABBIX_ACTIVE" => "Zabbix agent items are not converted: the \
             DumbMonit agent collects the usual system metrics itself"
            .into(),
        "SNMP_TRAP" => "SNMP trap items are not converted: DumbMonit receives traps itself".into(),
        "CALCULATED" => "calculated items are not converted".into(),
        other => format!("{other} items are not supported: only SNMP and HTTP agent items are"),
    }
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

pub(super) fn convert(doc: &Value, draft: &mut Draft) -> Result<(), ImportError> {
    let export =
        doc.get("zabbix_export").ok_or_else(|| ImportError("no zabbix_export section".into()))?;
    let templates = list(export, "templates");
    let Some(first) = templates.first() else {
        return Err(ImportError(
            "this Zabbix export holds no template (host exports are not supported)".into(),
        ));
    };
    let version = text(export, "version");
    let name = display_name(first);
    draft.identify(&name, &format!("Imported from the Zabbix template \"{name}\"."));
    if !(version.starts_with("6.") || version.starts_with("7.")) {
        draft.partial(
            "export",
            &version,
            "only 6.0 and 7.0 exports are supported; converted on a best-effort basis",
        );
    }

    let mut ctx = Ctx::default();
    let maps = list(export, "value_maps").iter();
    for map in maps.chain(templates.iter().flat_map(|t| list(t, "valuemaps"))) {
        let states: Vec<(String, String)> = list(map, "mappings")
            .iter()
            .map(|m| (text(m, "value"), text(m, "newvalue")))
            .filter(|(value, _)| !value.is_empty())
            .collect();
        ctx.valuemaps.entry(text(map, "name")).or_insert(states);
    }
    for template in templates {
        for entry in list(template, "macros") {
            let name = base_macro(&text(entry, "macro"));
            if matches!(text(entry, "type").as_str(), "SECRET_TEXT" | "VAULT") {
                ctx.secret.push(name.clone());
            }
            ctx.macros.entry(name).or_insert_with(|| text(entry, "value"));
        }
    }

    for template in templates {
        convert_template(&mut ctx, draft, template);
    }
    triggers::convert(&ctx, draft, export, templates);
    for key in ["graphs", "dashboards", "maps", "media_types"] {
        let count = list(export, key).len();
        if count > 0 {
            draft.ignored(key, &format!("{count} {key}"), "DumbMonit draws its own views");
        }
    }
    if ctx.macros.keys().any(|m| ctx.is_secret(m)) {
        draft.partial(
            "macro",
            "secret macros",
            "secret values are never imported: set the credential on each device",
        );
    }
    Ok(())
}

fn display_name(template: &Value) -> String {
    let name = text(template, "name");
    if name.is_empty() { text(template, "template") } else { name }
}

fn convert_template(ctx: &mut Ctx, draft: &mut Draft, template: &Value) {
    let profile = display_name(template);
    let items = list(template, "items");
    let by_key: BTreeMap<String, &Value> =
        items.iter().map(|item| (text(item, "key"), item)).collect();

    // Les maîtres d'abord : un item dépendant peut les précéder dans l'export.
    for item in items {
        let key = text(item, "key");
        match item_type(item).as_str() {
            "SNMP_AGENT" => snmp::item(ctx, draft, &profile, item),
            "HTTP_AGENT" => http::item(ctx, draft, item, items),
            "DEPENDENT" => {}
            other => draft.ignored("item", &key, unsupported(other)),
        }
    }
    for item in items.iter().filter(|item| item_type(item) == "DEPENDENT") {
        let key = text(item, "key");
        let master_key = item.get("master_item").map(|m| text(m, "key")).unwrap_or_default();
        match by_key.get(&master_key).map(|master| item_type(master)).as_deref() {
            Some("SNMP_AGENT") => snmp::dependent(ctx, draft, &profile, item),
            Some("HTTP_AGENT") => http::dependent(ctx, draft, item, &master_key),
            Some(other) => draft.ignored(
                "item",
                &key,
                format!("depends on a {other} item, which is not converted"),
            ),
            None => draft.ignored("item", &key, "its master item is not in this template"),
        }
    }

    for rule in list(template, "discovery_rules") {
        snmp::discovery(ctx, draft, &profile, rule, &by_key);
    }
    for link in list(template, "templates") {
        draft.ignored(
            "template link",
            &text(link, "name"),
            "linked template: export and import it separately",
        );
    }
    let scenarios = list(template, "httptests").len();
    if scenarios > 0 {
        draft.ignored(
            "web scenario",
            &format!("{scenarios} web scenarios"),
            "use HTTP monitors in DumbMonit",
        );
    }
    for key in ["dashboards", "graphs"] {
        let count = list(template, key).len();
        if count > 0 {
            draft.ignored(key, &format!("{count} {key}"), "DumbMonit draws its own views");
        }
    }
}

/// Retient la série produite pour un item, que ses déclencheurs citeront.
fn remember(ctx: &mut Ctx, key: &str, name: &str, counter: bool, http: bool, unit: &str) {
    ctx.metrics.insert(
        key.to_string(),
        MetricRef { full: format!("dumbmonit_{name}"), counter, http, unit: unit.to_string() },
    );
}

/// Les étiquettes d'une découverte, de `{#IFNAME}` vers `ifname`.
fn label_of(macro_name: &str) -> String {
    let name = snake(macro_name.trim_start_matches("{#").trim_end_matches('}'));
    if name == "index" { "index_label".into() } else { name }
}

// ---------------------------------------------------------------------------
// XML
// ---------------------------------------------------------------------------

/// Lit un export XML en arbre de même forme que l'export YAML : les
/// conteneurs au pluriel (`<items><item>…`) deviennent des listes.
pub(in crate::import) fn xml_to_value(text: &str) -> Result<Value, ImportError> {
    let document = roxmltree::Document::parse(text)
        .map_err(|error| ImportError(format!("invalid XML: {error}")))?;
    let root = document.root_element();
    if root.tag_name().name() != "zabbix_export" {
        return Err(ImportError("this XML file is not a Zabbix export (zabbix_export)".into()));
    }
    Ok(json!({ "zabbix_export": element(root, 0) }))
}

fn element(node: roxmltree::Node<'_, '_>, depth: usize) -> Value {
    if depth > 32 {
        return Value::Null;
    }
    let children: Vec<_> = node.children().filter(roxmltree::Node::is_element).collect();
    let Some(first) = children.first() else {
        return Value::String(node.text().unwrap_or_default().to_string());
    };
    let tag = node.tag_name().name();
    let child = first.tag_name().name();
    let same = children.iter().all(|c| c.tag_name().name() == child);
    let plural = tag == format!("{child}s")
        || child.strip_suffix('y').is_some_and(|stem| tag == format!("{stem}ies"))
        || (tag == "preprocessing" && child == "step");
    if same && plural {
        return Value::Array(children.iter().map(|c| element(*c, depth + 1)).collect());
    }
    let mut map = Map::new();
    for c in &children {
        map.insert(c.tag_name().name().to_string(), element(*c, depth + 1));
    }
    Value::Object(map)
}
