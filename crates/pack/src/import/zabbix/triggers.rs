//! Déclencheurs Zabbix simples → règles d'alerte du paquet.
//!
//! Seule forme reconnue : `fonction(/hôte/clé[,fenêtre]) opérateur valeur`, avec
//! `last`, `avg`, `min` ou `max`, un opérateur `>`, `>=`, `<` ou `<=`, et une
//! constante ou une macro `{$X}`. Le reste est rapporté, jamais deviné.

use serde_json::{Map, Value, json};

use super::{Ctx, list, text};
use crate::import::out::{Draft, clean_text};

/// Une comparaison lue dans une expression.
#[derive(Debug, PartialEq, Eq)]
struct Comparison {
    function: String,
    key: String,
    window: String,
    op: String,
    value: String,
}

fn parse(expression: &str) -> Result<Comparison, String> {
    let expr = expression.trim();
    let lowered = expr.to_ascii_lowercase();
    if [" and ", " or ", "not "].iter().any(|word| lowered.contains(word)) {
        return Err("combines several conditions".into());
    }
    let open = expr.find('(').ok_or("not a function call")?;
    let function = expr[..open].trim().to_string();
    let rest = expr[open + 1..].strip_prefix('/').ok_or("old (pre-6.0) expression syntax")?;
    let host_end = rest.find('/').ok_or("no item key")?;
    let rest = &rest[host_end + 1..];

    // La clé peut porter des crochets, des virgules et des guillemets.
    let (mut depth, mut quoted, mut end) = (0usize, false, None);
    for (at, c) in rest.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' if !quoted => depth += 1,
            ']' if !quoted => depth = depth.saturating_sub(1),
            ',' | ')' if !quoted && depth == 0 => {
                end = Some(at);
                break;
            }
            _ => {}
        }
    }
    let end = end.ok_or("unterminated function call")?;
    let key = rest[..end].trim().to_string();
    let rest = &rest[end..];
    let close = rest.find(')').ok_or("unterminated function call")?;
    let window = rest[..close].trim_start_matches(',').trim().to_string();
    let tail = rest[close + 1..].trim();

    let op = [">=", "<=", "<>", ">", "<", "="]
        .into_iter()
        .find(|op| tail.starts_with(op))
        .ok_or("no comparison after the function")?;
    let value = tail[op.len()..].trim().to_string();
    let arithmetic = value.chars().skip(1).any(|c| "+*/()".contains(c) || c.is_whitespace());
    if value.is_empty() || arithmetic {
        return Err("compares with an expression, not a value".into());
    }
    Ok(Comparison { function, key, window, op: op.to_string(), value })
}

/// Un nombre Zabbix, suffixes compris (`10K`, `5m`, `1G`).
fn number(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    if let Ok(value) = raw.parse::<f64>() {
        return Some(value);
    }
    let suffix = raw.chars().last()?;
    let body = &raw[..raw.len() - suffix.len_utf8()];
    let factor = match suffix {
        'K' => 1024.0,
        'M' => 1024.0 * 1024.0,
        'G' => 1024f64.powi(3),
        'T' => 1024f64.powi(4),
        's' => 1.0,
        'm' => 60.0,
        'h' => 3600.0,
        'd' => 86_400.0,
        'w' => 604_800.0,
        _ => return None,
    };
    body.parse::<f64>().ok().map(|value| value * factor)
}

/// Une fenêtre de temps Zabbix (`5m`, `300`, `1h`) en durée MetricsQL.
fn window(raw: &str) -> Option<String> {
    let raw = raw.trim();
    // Une fenêtre en nombre de valeurs (`#3`) n'a pas d'équivalent ; K, M, G et
    // T sont des tailles, pas des durées.
    if raw.starts_with('#') || raw.ends_with(['K', 'M', 'G', 'T']) {
        return None;
    }
    let seconds = number(raw).filter(|seconds| *seconds >= 1.0)?;
    Some(format!("{}s", seconds.round() as u64))
}

/// Vrai si le champ porte quelque chose (texte ou liste non vides).
fn present(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        _ => false,
    }
}

fn severity(priority: &str) -> &'static str {
    match priority {
        "HIGH" | "DISASTER" => "critical",
        "WARNING" | "AVERAGE" => "warning",
        _ => "info",
    }
}

pub(super) fn convert(ctx: &Ctx, draft: &mut Draft, export: &Value, templates: &[Value]) {
    for template in templates {
        for item in list(template, "items") {
            for trigger in list(item, "triggers") {
                one(ctx, draft, trigger, "trigger");
            }
        }
        for rule in list(template, "discovery_rules") {
            for proto in list(rule, "item_prototypes") {
                for trigger in list(proto, "trigger_prototypes") {
                    one(ctx, draft, trigger, "trigger prototype");
                }
            }
            for trigger in list(rule, "trigger_prototypes") {
                one(ctx, draft, trigger, "trigger prototype");
            }
        }
    }
    for trigger in list(export, "triggers") {
        one(ctx, draft, trigger, "trigger");
    }
}

fn one(ctx: &Ctx, draft: &mut Draft, trigger: &Value, element: &str) {
    let name = text(trigger, "name");
    match rule(ctx, draft, trigger) {
        Ok((rule, notes)) => {
            if draft.push_rule(&clean_text(&name), rule, element) {
                if notes.is_empty() {
                    draft.converted(element, &name, "alert rule");
                } else {
                    draft.partial(element, &name, notes.join("; "));
                }
            }
        }
        Err(reason) => draft.ignored(element, &name, reason),
    }
}

fn rule(
    ctx: &Ctx,
    draft: &mut Draft,
    trigger: &Value,
) -> Result<(Map<String, Value>, Vec<String>), String> {
    let found = parse(&text(trigger, "expression"))?;
    let metric = ctx
        .metrics
        .get(&found.key)
        .ok_or_else(|| format!("its item {} was not converted", found.key))?;
    if !matches!(found.op.as_str(), ">" | ">=" | "<" | "<=") {
        return Err(format!("operator {} is not supported (only >, >=, < and <=)", found.op));
    }
    let mut notes = Vec::new();

    let span = match (found.function.as_str(), found.window.as_str()) {
        ("last", "" | "#1") => None,
        ("last", other) => return Err(format!("last() with {other} is not supported")),
        ("avg" | "min" | "max", raw) => {
            Some(window(raw).ok_or_else(|| format!("window \"{raw}\" is not a duration"))?)
        }
        (other, _) => return Err(format!("function {other}() is not supported")),
    };
    let series = &metric.full;
    let expr = match (metric.counter, span) {
        (true, span) => {
            notes.push("compares the per-second rate".to_string());
            format!("rate({series}[{}])", span.unwrap_or_else(|| "5m".into()))
        }
        (false, None) => series.clone(),
        (false, Some(span)) => format!("{}_over_time({series}[{span}])", found.function),
    };

    let threshold = if found.value.starts_with("{$") {
        let default = ctx
            .resolve(&found.value)
            .ok_or_else(|| format!("macro {} has no default value", found.value))?;
        let parsed = number(default)
            .ok_or_else(|| format!("macro {} is not a number: {default}", found.value))?;
        if metric.http && default.trim().parse::<f64>().is_ok() {
            let key = draft.option_for(&super::base_macro(&found.value), default.trim());
            notes.push(format!("threshold set per device by the option {key}"));
            json!(format!("{{{{option.{key}}}}}"))
        } else {
            notes.push(format!("macro {} replaced by its default {default}", found.value));
            json!(parsed)
        }
    } else {
        let value = number(&found.value);
        json!(value.ok_or_else(|| format!("\"{}\" is not a number", found.value))?)
    };

    for (key, note) in [
        ("recovery_expression", "recovery expression ignored: the alert clears with the condition"),
        ("dependencies", "dependencies ignored: DumbMonit suppresses by parent device"),
    ] {
        if present(trigger.get(key)) {
            notes.push(note.to_string());
        }
    }

    let mut rule = Map::new();
    rule.insert("title".into(), json!(clean_text(&text(trigger, "name"))));
    let description: String = text(trigger, "description").chars().take(500).collect();
    if !description.is_empty() {
        rule.insert("description".into(), json!(description));
    }
    rule.insert("expr".into(), json!(expr));
    rule.insert("op".into(), json!(found.op));
    rule.insert("threshold".into(), threshold);
    rule.insert("severity".into(), json!(severity(&text(trigger, "priority"))));
    if !metric.unit.is_empty() {
        rule.insert("unit".into(), json!(metric.unit));
    }
    Ok((rule, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_expression_simple_se_lit() {
        let found = parse("avg(/Acme UPS/ups.load[\"a,b\"],5m)>={$LOAD.MAX}").unwrap();
        assert_eq!(found.function, "avg");
        assert_eq!(found.key, "ups.load[\"a,b\"]");
        assert_eq!(found.window, "5m");
        assert_eq!(found.op, ">=");
        assert_eq!(found.value, "{$LOAD.MAX}");
        assert!(parse("last(/T/a)>1 and last(/T/b)<2").is_err());
        assert!(parse("{Tpl:key.last()}>5").is_err());
        assert!(parse("last(/T/a)>last(/T/b)*2").is_err());
        assert_eq!(number("10K"), Some(10240.0));
        assert_eq!(window("5m").as_deref(), Some("300s"));
        assert_eq!(window("#3"), None);
    }
}
