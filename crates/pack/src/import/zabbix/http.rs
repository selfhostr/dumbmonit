//! Items HTTP agent d'un template Zabbix : chaque item maître devient une
//! source du paquet, ses items dépendants (JSONPath ou expression régulière)
//! des métriques de cette source.

use serde_json::{Map, Value, json};

use super::{
    Ctx, Step, is_text_value, item_label, item_type, key_stem, list, remember, shape, steps, text,
    unit,
};
use crate::MAX_REQUESTS;
use crate::import::out::Draft;

/// Macros d'hôte qui désignent l'adresse de l'équipement.
const HOST_MACROS: &[&str] =
    &["{HOST.CONN}", "{HOST.IP}", "{HOST.DNS}", "{HOST.HOST}", "{HOST.NAME}"];

/// Étapes que [`shape`] sait traduire.
const NUMERIC_STEPS: &[&str] = &[
    "MULTIPLIER",
    "CHANGE_PER_SECOND",
    "SIMPLE_CHANGE",
    "DISCARD_UNCHANGED",
    "DISCARD_UNCHANGED_HEARTBEAT",
    "CHECK_NOT_SUPPORTED",
];

/// Une URL Zabbix découpée.
#[derive(Debug, Default, PartialEq, Eq)]
struct Url {
    scheme: Option<&'static str>,
    port: Option<u16>,
    /// Hôte fixe écrit dans le template (et non l'adresse de l'équipement).
    fixed_host: Option<String>,
    /// Chemin et requête, macros comprises.
    path: String,
}

fn split_url(ctx: &Ctx, raw: &str) -> Result<Url, String> {
    let raw = raw.trim();
    let (scheme, rest) = match raw.find("://") {
        Some(at) => (ctx.expand(&raw[..at]).to_ascii_lowercase(), &raw[at + 3..]),
        None => (String::new(), raw),
    };
    let scheme = match scheme.as_str() {
        "" => None,
        "http" => Some("http"),
        "https" => Some("https"),
        other => return Err(format!("scheme \"{other}\" is not http or https")),
    };
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    let path = if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('?') {
        format!("/{path}")
    } else {
        path.to_string()
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains('}') || port.starts_with("{$") => {
            let port = ctx.expand(port);
            let port: u16 =
                port.trim().parse().map_err(|_| format!("port \"{port}\" is not a number"))?;
            (host, Some(port))
        }
        _ => (authority, None),
    };
    let is_device = HOST_MACROS.contains(&host) || host.starts_with("{$");
    let fixed_host = (!is_device && !host.is_empty()).then(|| ctx.expand(host));
    Ok(Url { scheme, port, fixed_host, path })
}

/// Les macros utilisateur citées par un texte (`{$X}`).
fn macros_in(raw: &str) -> Vec<String> {
    raw.match_indices("{$")
        .filter_map(|(start, _)| {
            raw[start..].find('}').map(|end| raw[start..=start + end].to_string())
        })
        .collect()
}

/// Remplace les macros `{$X}` par des options ; refuse toute autre macro.
fn template(ctx: &Ctx, draft: &mut Draft, raw: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let end = rest[start..].find('}').ok_or("unbalanced braces")?;
        let token = &rest[start..start + end + 1];
        if !token.starts_with("{$") {
            return Err(format!("uses the macro {token}, which has no equivalent"));
        }
        if ctx.is_secret(token) {
            return Err(format!("puts the secret macro {token} in the URL or body"));
        }
        let key = draft.option_for(&super::base_macro(token), ctx.resolve(token).unwrap_or(""));
        out.push_str(&format!("{{{{option.{key}}}}}"));
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Comment l'item maître lira sa réponse, d'après ses dépendants.
fn format_for(item: &Value, key: &str, items: &[Value]) -> &'static str {
    let first_step = |candidate: &Value| steps(candidate).first().map(|s| s.kind.clone());
    let mut kinds: Vec<Option<String>> = vec![first_step(item)];
    for other in items {
        let master = other.get("master_item").map(|m| text(m, "key")).unwrap_or_default();
        if item_type(other) == "DEPENDENT" && master == key {
            kinds.push(first_step(other));
        }
    }
    if kinds.iter().any(|k| k.as_deref() == Some("JSONPATH")) { "json" } else { "text" }
}

/// Un item `HTTP_AGENT` : une source, et une métrique s'il porte lui-même un nombre.
pub(super) fn item(ctx: &mut Ctx, draft: &mut Draft, item: &Value, items: &[Value]) {
    let key = text(item, "key");
    match source(ctx, draft, item, items) {
        Ok((id, format, notes)) => {
            let reason = if notes.is_empty() {
                format!("HTTP source {id}")
            } else {
                format!("HTTP source {id}; {}", notes.join("; "))
            };
            draft.partial("item", &key, reason);
            ctx.sources.insert(key.clone(), (id.clone(), format.to_string()));
            if !is_text_value(item) {
                metric(ctx, draft, item, &id, format, &steps(item));
            }
        }
        Err(reason) => draft.ignored("item", &key, reason),
    }
}

fn source(
    ctx: &Ctx,
    draft: &mut Draft,
    item: &Value,
    items: &[Value],
) -> Result<(String, &'static str, Vec<String>), String> {
    if draft.http.sources.len() >= MAX_REQUESTS {
        return Err(format!("more than {MAX_REQUESTS} HTTP sources in one pack"));
    }
    let key = text(item, "key");
    let mut notes = Vec::new();
    let url = split_url(ctx, &text(item, "url"))?;
    let method = match text(item, "request_method").as_str() {
        "" | "GET" => "GET",
        "POST" => "POST",
        other => return Err(format!("request method {other} is not supported")),
    };
    if !matches!(text(item, "retrieve_mode").as_str(), "" | "BODY") {
        return Err("only the response body can be read".into());
    }
    let (auth, credential) = match text(item, "authtype").as_str() {
        "" | "NONE" => ("none", None),
        "BASIC" => ("basic", Some("username_password")),
        "BEARER" => ("bearer", Some("api_token")),
        other => return Err(format!("{other} authentication is not supported")),
    };

    let mut headers = Map::new();
    let mut header_credential = None;
    for header in list(item, "headers") {
        let (name, value) = (text(header, "name"), text(header, "value"));
        let secret = macros_in(&value).iter().any(|token| ctx.is_secret(token));
        let value = if secret {
            header_credential = Some("api_token");
            notes.push(format!("header {name} carries the device's API token"));
            "{{credential.token}}".to_string()
        } else {
            template(ctx, draft, &value)?
        };
        headers.insert(name, json!(value));
    }
    let credential = match (credential, header_credential) {
        (Some(_), Some(_)) => return Err("both a login and a token header".into()),
        (one, other) => one.or(other),
    };
    if let Some(wanted) = credential
        && draft.http.credential.is_some_and(|known| known != wanted)
    {
        return Err("another credential kind than the other sources of this pack".into());
    }

    let mut path = template(ctx, draft, &url.path)?;
    let mut query = Vec::new();
    for field in list(item, "query_fields") {
        let value = template(ctx, draft, &text(field, "value"))?;
        query.push(format!("{}={value}", text(field, "name")));
    }
    if !query.is_empty() {
        path.push(if path.contains('?') { '&' } else { '?' });
        path.push_str(&query.join("&"));
    }
    if let Some(host) = &url.fixed_host {
        notes.push(format!("the template calls the fixed host {host}: use it as the address"));
    }
    if credential == Some("username_password") {
        notes.push("the login is set on each device, never imported".into());
    }

    let id = draft.source_id(&key_stem(&key));
    let format = format_for(item, &key, items);
    let mut value = json!({ "id": id, "type": "http", "path": path, "format": format });
    value["auth"] = json!(if header_credential.is_some() { "none" } else { auth });
    if method == "POST" {
        value["method"] = json!("POST");
        let body = text(item, "posts");
        if !body.is_empty() {
            value["body"] = json!(template(ctx, draft, &body)?);
        }
    }
    if !headers.is_empty() {
        value["headers"] = Value::Object(headers);
    }
    draft.http.sources.push(value);
    if credential.is_some() {
        draft.http.credential = credential;
    }
    if draft.http.scheme.is_none() {
        draft.http.scheme = url.scheme;
    }
    if draft.http.default_port.is_none() {
        draft.http.default_port = url.port;
    }
    Ok((id, format, notes))
}

/// Un item dépendant d'un item HTTP maître déjà converti.
pub(super) fn dependent(ctx: &mut Ctx, draft: &mut Draft, item: &Value, master: &str) {
    let key = text(item, "key");
    let Some((source, format)) = ctx.sources.get(master).cloned() else {
        draft.ignored("item", &key, "its HTTP master item was not converted");
        return;
    };
    let format = if format == "json" { "json" } else { "text" };
    metric(ctx, draft, item, &source, format, &steps(item));
}

/// Chemin JSONPath Zabbix → RFC 9535 : `.first()` retiré, autres fonctions refusées.
fn json_path(raw: &str, notes: &mut Vec<String>) -> Result<String, String> {
    let mut path = raw.trim().to_string();
    if let Some(stripped) = path.strip_suffix(".first()") {
        notes.push("JSONPath .first() dropped: every match makes a series".into());
        path = stripped.to_string();
    }
    if path.ends_with("()") {
        return Err(format!("JSONPath function in {raw} is not supported"));
    }
    serde_json_path::JsonPath::parse(&path).map_err(|e| format!("JSONPath {raw}: {e}"))?;
    Ok(path)
}

fn metric(
    ctx: &mut Ctx,
    draft: &mut Draft,
    item: &Value,
    source: &str,
    format: &str,
    all: &[Step],
) -> bool {
    let key = text(item, "key");
    if is_text_value(item) {
        draft.ignored("item", &key, "text value: DumbMonit records numbers");
        return false;
    }
    let mut notes = Vec::new();
    let extractor = match all.first().map(|s| (s.kind.as_str(), s)) {
        Some(("JSONPATH", step)) if format == "json" => {
            json_path(step.params.first().map_or("", String::as_str), &mut notes)
                .map(|path| ("json", path, 1))
        }
        Some(("REGEX", step)) if format == "text" => {
            let pattern = step.params.first().cloned().unwrap_or_default();
            let output = step.params.get(1).map_or("", String::as_str).trim();
            let groups = regex::Regex::new(&pattern).map(|r| r.captures_len()).unwrap_or(0);
            if output == "\\1" && groups > 1 {
                Ok(("regex", pattern, 1))
            } else {
                Err("REGEX steps convert only with the output \\1 of a valid pattern".into())
            }
        }
        Some(("JSONPATH" | "REGEX", _)) => Err("the source is read in another format".into()),
        Some((kind, _)) if !NUMERIC_STEPS.contains(&kind) => {
            Err(format!("preprocessing step {kind} is not supported"))
        }
        _ if format == "text" => Ok(("regex", r"(-?[0-9]+(?:\.[0-9]+)?)".to_string(), 0)),
        _ => Err("needs a JSONPATH step to read the JSON response".into()),
    };
    let (field, expression, skip) = match extractor {
        Ok(found) => found,
        Err(reason) => {
            draft.ignored("item", &key, reason);
            return false;
        }
    };
    let shape = match shape(ctx, &all[skip..]) {
        Ok(shape) => shape,
        Err(reason) => {
            draft.ignored("item", &key, reason);
            return false;
        }
    };
    notes.extend(shape.notes);
    let help = item_label(ctx, item, &mut notes);
    let unit = unit(&text(item, "units"), shape.counter);
    let full = draft.metric_name(&key_stem(&key));
    // Les métriques HTTP s'écrivent sans le préfixe, que le paquet ajoute.
    let name = full.strip_prefix(&draft.prefix()).unwrap_or(&full).to_string();
    let mut value = json!({
        "name": name,
        "help": help,
        "source": source,
        "kind": if shape.counter { "counter" } else { "gauge" },
    });
    value[field] = json!(expression);
    if let Some(scale) = shape.scale {
        value["scale"] = json!(scale);
    }
    if !draft.push_http_metric(value, "item", &key) {
        return false;
    }
    remember(ctx, &key, &full, shape.counter, true, &unit);
    if notes.is_empty() {
        draft.converted("item", &key, format!("HTTP metric {full}"));
    } else {
        draft.partial("item", &key, notes.join("; "));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_url_zabbix_se_decoupe() {
        let mut ctx = Ctx::default();
        ctx.macros.insert("{$PORT}".into(), "8080".into());
        let url = split_url(&ctx, "https://{HOST.CONN}:{$PORT}/api/v1/status?x=1").unwrap();
        assert_eq!(url.scheme, Some("https"));
        assert_eq!(url.port, Some(8080));
        assert_eq!(url.fixed_host, None);
        assert_eq!(url.path, "/api/v1/status?x=1");
        let fixed = split_url(&ctx, "http://status.example.net/health").unwrap();
        assert_eq!(fixed.fixed_host.as_deref(), Some("status.example.net"));
        assert!(split_url(&ctx, "ftp://{HOST.CONN}/").is_err());
    }
}
