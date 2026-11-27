//! Lecture de la sortie d'un contrôle : greffon Nagios, JSON, texte Prometheus.
//!
//! Fonctions pures, sans entrée-sortie : l'agent leur donne ce qu'il a lu du
//! processus (déjà borné à [`CHECK_OUTPUT_MAX_BYTES`](super::CHECK_OUTPUT_MAX_BYTES)),
//! elles rendent un statut, un message et des mesures. Une sortie illisible
//! n'est jamais une panique : elle devient un UNKNOWN qui dit pourquoi.

use std::collections::BTreeSet;

use super::{
    CheckMetric, FORMAT_JSON, FORMAT_NAGIOS, FORMAT_PROMETHEUS, MAX_CHECK_MESSAGE,
    MAX_CHECK_METRICS, MAX_METRIC_NAME, STATUS_CRITICAL, STATUS_OK, STATUS_UNKNOWN,
    STATUS_WARNING, status_from_exit, strip_controls, truncate_chars,
};

/// Ce qu'une sortie dit du contrôle.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedOutput {
    pub status: u8,
    pub message: String,
    pub metrics: Vec<CheckMetric>,
}

/// Lit la sortie selon le format du contrôle. `None` pour un format inconnu.
pub fn parse_output(
    format: &str,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> Option<ParsedOutput> {
    let mut parsed = match format {
        FORMAT_NAGIOS => parse_nagios(exit_code, stdout),
        FORMAT_JSON => parse_json(exit_code, stdout),
        FORMAT_PROMETHEUS => parse_prometheus(exit_code, stdout),
        _ => return None,
    };
    // Un greffon muet sur sa sortie standard parle souvent sur l'erreur.
    if parsed.message.is_empty() {
        let line = stderr.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
        parsed.message = message(line);
    }
    Some(parsed)
}

/// Sortie d'un greffon Nagios (guide des greffons, « Plugin Output ») :
///
/// ```text
/// DISK OK - free space: / 3326 MB (56%) | /=2643MB;5948;5958;0;5968
/// / 15272 MB (77%);
/// /boot 68 MB (69%); | /boot=68MB;88;93;0;98
/// /home=69357MB;253404;253409;0;253414
/// ```
///
/// La première ligne donne le message (avant `|`) et des données de
/// performance (après). Les lignes suivantes sont du texte long jusqu'à la
/// première qui contient `|` : ce qui la suit, et toutes les lignes après,
/// sont encore des données de performance. Le statut est le code de sortie.
pub fn parse_nagios(exit_code: Option<i32>, stdout: &str) -> ParsedOutput {
    let mut lines = stdout.lines();
    let first = lines.next().unwrap_or("");
    let (text, mut perf) = match first.split_once('|') {
        Some((text, perf)) => (text, perf.to_string()),
        None => (first, String::new()),
    };
    let mut in_perf = false;
    for line in lines {
        if in_perf {
            perf.push(' ');
            perf.push_str(line);
        } else if let Some((_, rest)) = line.split_once('|') {
            in_perf = true;
            perf.push(' ');
            perf.push_str(rest);
        }
    }
    ParsedOutput {
        status: status_from_exit(exit_code),
        message: message(text),
        metrics: parse_perfdata(&perf),
    }
}

/// Données de performance : `'libellé'=valeur[UOM];[warn];[crit];[min];[max]`,
/// séparées par des espaces. Le libellé se met entre apostrophes s'il contient
/// une espace ou un `=` ; `''` y représente une apostrophe.
///
/// Les unités sont ramenées à la base : `ms` et `us` en secondes, `KB`, `MB`,
/// `GB`, `TB` en octets (puissances de 1024, comme les greffons), `c` marque un
/// compteur. Une valeur `U` (indéterminée) ou illisible est écartée, le reste
/// de la ligne est gardé. Un libellé vu deux fois ne compte qu'une fois.
pub fn parse_perfdata(text: &str) -> Vec<CheckMetric> {
    let mut metrics = Vec::new();
    let mut seen = BTreeSet::new();
    let mut rest = text.trim_start();
    while !rest.is_empty() && metrics.len() < MAX_CHECK_METRICS {
        let (label, after) = match take_label(rest) {
            Some(found) => found,
            None => break,
        };
        // La valeur va jusqu'à la prochaine espace.
        let end = after.find(char::is_whitespace).unwrap_or(after.len());
        let (data, next) = after.split_at(end);
        rest = next.trim_start();
        if let Some(metric) = perf_metric(&label, data)
            && seen.insert(metric.name.clone())
        {
            metrics.push(metric);
        }
    }
    metrics
}

/// Le libellé en tête de `text`, et ce qui suit son `=`.
///
/// Sans apostrophes, le libellé s'arrête au premier `=` ; un mot sans `=`
/// (texte parasite) est sauté jusqu'au suivant. Une boucle, pas une
/// récursion : 64 Kio de mots parasites ne doivent pas épuiser la pile.
fn take_label(text: &str) -> Option<(String, &str)> {
    let mut text = text;
    loop {
        if let Some(quoted) = text.strip_prefix('\'') {
            return quoted_label(quoted);
        }
        let word_end = text.find(char::is_whitespace).unwrap_or(text.len());
        if let Some((label, _)) = text[..word_end].split_once('=')
            && !label.is_empty()
        {
            return Some((label.to_string(), &text[label.len() + 1..]));
        }
        text = text[word_end..].trim_start();
        if text.is_empty() {
            return None;
        }
    }
}

/// Un libellé entre apostrophes (la première déjà ôtée), `''` valant une
/// apostrophe ; il faut un `=` juste après l'apostrophe fermante.
fn quoted_label(quoted: &str) -> Option<(String, &str)> {
    let mut label = String::new();
    let mut chars = quoted.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '\'' {
            label.push(c);
            continue;
        }
        if chars.peek().map(|(_, next)| *next) == Some('\'') {
            label.push('\'');
            chars.next();
            continue;
        }
        return quoted[i + 1..].strip_prefix('=').map(|value| (label, value));
    }
    None
}

/// Une donnée de performance, unité normalisée.
fn perf_metric(label: &str, data: &str) -> Option<CheckMetric> {
    let name = truncate_chars(&strip_controls(label.trim()), MAX_METRIC_NAME);
    if name.is_empty() {
        return None;
    }
    let mut fields = data.split(';');
    let raw_value = fields.next()?;
    let split = raw_value
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(raw_value.len());
    // `1e3` est un nombre, mais un `e` seul en fin de valeur ne l'est pas : on
    // recule jusqu'à la dernière coupure qui donne un nombre.
    let (value, uom) = split_number(raw_value, split)?;
    let (factor, unit, counter) = unit_of(uom)?;
    let bound = |raw: Option<&str>| raw.and_then(threshold).map(|value| value * factor);
    Some(CheckMetric {
        name,
        value: value * factor,
        unit: unit.to_string(),
        counter,
        warn: bound(fields.next()),
        crit: bound(fields.next()),
        min: bound(fields.next()),
        max: bound(fields.next()),
    })
}

/// Coupe `raw` en nombre et unité, au plus tard à `split`.
fn split_number(raw: &str, split: usize) -> Option<(f64, &str)> {
    let mut cut = split;
    while cut > 0 {
        if let Some(number) = number(&raw[..cut]) {
            return Some((number, &raw[cut..]));
        }
        cut = raw[..cut].char_indices().next_back().map_or(0, |(i, _)| i);
    }
    None
}

/// Un nombre fini ; la virgule décimale de certaines langues est acceptée.
fn number(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let parsed = raw.parse::<f64>().ok().or_else(|| {
        (raw.matches(',').count() == 1 && !raw.contains('.'))
            .then(|| raw.replace(',', "."))
            .and_then(|fixed| fixed.parse::<f64>().ok())
    })?;
    parsed.is_finite().then_some(parsed)
}

/// Facteur, unité normalisée et nature d'une unité Nagios. `None` : unité
/// inconnue, la donnée est écartée plutôt que mal lue.
fn unit_of(uom: &str) -> Option<(f64, &'static str, bool)> {
    const KIB: f64 = 1024.0;
    Some(match uom {
        "" => (1.0, "", false),
        "%" => (1.0, "%", false),
        "s" => (1.0, "s", false),
        "ms" => (1e-3, "s", false),
        "us" => (1e-6, "s", false),
        "ns" => (1e-9, "s", false),
        "B" => (1.0, "B", false),
        "KB" | "kB" | "KiB" => (KIB, "B", false),
        "MB" | "MiB" => (KIB * KIB, "B", false),
        "GB" | "GiB" => (KIB * KIB * KIB, "B", false),
        "TB" | "TiB" => (KIB * KIB * KIB * KIB, "B", false),
        "c" => (1.0, "", true),
        _ => return None,
    })
}

/// Le seuil d'un champ `warn` ou `crit`. Un nombre seul est pris tel quel ;
/// une plage (`10:20`, `~:10`, `@5:`, `10:`) donne sa borne haute, ou sa borne
/// basse quand elle n'en a pas. Vide ou illisible : pas de seuil.
fn threshold(raw: &str) -> Option<f64> {
    let raw = raw.trim().trim_start_matches('@');
    match raw.split_once(':') {
        None => number(raw),
        Some((low, high)) => number(high).or_else(|| number(low.trim_start_matches('~'))),
    }
}

/// Le message d'un contrôle : une ligne, bornée, sans caractère de contrôle.
fn message(text: &str) -> String {
    truncate_chars(strip_controls(text).trim(), MAX_CHECK_MESSAGE)
}

/// Sortie JSON d'un script :
///
/// ```json
/// {"status": "warning", "message": "3 jobs late", "metrics": {"late_jobs": 3}}
/// ```
///
/// `status` vaut `ok`, `warning`, `critical` ou `unknown` (ou `up`, `degraded`,
/// `down`, ou leur code 0 à 3) ; absent, c'est le code de sortie qui décide.
/// Une mesure est un nombre, ou un objet `{"value": 3, "unit": "s", "warn": 5,
/// "crit": 10, "min": 0, "max": 100}`.
pub fn parse_json(exit_code: Option<i32>, stdout: &str) -> ParsedOutput {
    let value: serde_json::Value = match serde_json::from_str(stdout.trim()) {
        Ok(value) => value,
        Err(error) => {
            return ParsedOutput {
                status: STATUS_UNKNOWN,
                message: message(&format!("the script did not print valid JSON: {error}")),
                metrics: Vec::new(),
            };
        }
    };
    let Some(object) = value.as_object() else {
        return ParsedOutput {
            status: STATUS_UNKNOWN,
            message: "the script printed JSON that is not an object".to_string(),
            metrics: Vec::new(),
        };
    };
    let status = match object.get("status") {
        None | Some(serde_json::Value::Null) => Some(status_from_exit(exit_code)),
        Some(serde_json::Value::String(text)) => status_from_word(text),
        Some(serde_json::Value::Number(code)) => {
            code.as_u64().filter(|code| *code <= 3).map(|code| code as u8)
        }
        Some(_) => None,
    };
    let text = object.get("message").and_then(|m| m.as_str()).unwrap_or("");
    let mut metrics = Vec::new();
    if let Some(map) = object.get("metrics").and_then(|m| m.as_object()) {
        for (name, entry) in map.iter().take(MAX_CHECK_METRICS) {
            if let Some(metric) = json_metric(name, entry) {
                metrics.push(metric);
            }
        }
    }
    match status {
        Some(status) => ParsedOutput { status, message: message(text), metrics },
        None => ParsedOutput {
            status: STATUS_UNKNOWN,
            message: message(&format!("unknown status in the JSON output; {text}")),
            metrics,
        },
    }
}

fn status_from_word(word: &str) -> Option<u8> {
    match word.trim().to_ascii_lowercase().as_str() {
        "ok" | "up" | "pass" | "passing" => Some(STATUS_OK),
        "warning" | "warn" | "degraded" => Some(STATUS_WARNING),
        "critical" | "crit" | "down" | "fail" | "failed" | "error" => Some(STATUS_CRITICAL),
        "unknown" => Some(STATUS_UNKNOWN),
        _ => None,
    }
}

fn json_metric(name: &str, entry: &serde_json::Value) -> Option<CheckMetric> {
    let name = truncate_chars(&strip_controls(name.trim()), MAX_METRIC_NAME);
    if name.is_empty() {
        return None;
    }
    let finite = |value: Option<&serde_json::Value>| {
        value.and_then(serde_json::Value::as_f64).filter(|v| v.is_finite())
    };
    match entry {
        serde_json::Value::Number(_) => {
            finite(Some(entry)).map(|value| CheckMetric::gauge(name, value))
        }
        serde_json::Value::Object(fields) => {
            let value = finite(fields.get("value"))?;
            let unit = fields.get("unit").and_then(|u| u.as_str()).unwrap_or("");
            let (factor, unit, counter) = unit_of(unit).unwrap_or((1.0, "", false));
            let scaled = |key: &str| finite(fields.get(key)).map(|v| v * factor);
            Some(CheckMetric {
                name,
                value: value * factor,
                unit: unit.to_string(),
                counter: counter || fields.get("counter").and_then(|c| c.as_bool()) == Some(true),
                warn: scaled("warn"),
                crit: scaled("crit"),
                min: scaled("min"),
                max: scaled("max"),
            })
        }
        _ => None,
    }
}

/// Texte d'exposition Prometheus : `nom{étiquettes} valeur [horodatage]`, les
/// lignes `# TYPE nom counter` marquant les compteurs. Le statut est le code
/// de sortie ; chaque série devient une mesure nommée comme elle s'écrit
/// (`requests_total{code="200"}`).
pub fn parse_prometheus(exit_code: Option<i32>, stdout: &str) -> ParsedOutput {
    let mut counters = BTreeSet::new();
    let mut metrics = Vec::new();
    let mut seen = BTreeSet::new();
    let mut unreadable = 0usize;
    for line in stdout.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            let mut words = comment.split_whitespace();
            if words.next() == Some("TYPE")
                && let (Some(name), Some(kind)) = (words.next(), words.next())
                && kind == "counter"
            {
                counters.insert(name.to_string());
            }
            continue;
        }
        if metrics.len() >= MAX_CHECK_METRICS {
            break;
        }
        match prometheus_line(line) {
            Some((family, series, value)) => {
                if seen.insert(series.clone()) {
                    let mut metric = CheckMetric::gauge(series, value);
                    metric.counter = counters.contains(family)
                        || counters.contains(family.trim_end_matches("_total"));
                    metrics.push(metric);
                }
            }
            None => unreadable += 1,
        }
    }
    let text = if unreadable > 0 {
        format!("{unreadable} unreadable line(s) in the Prometheus output")
    } else {
        String::new()
    };
    ParsedOutput { status: status_from_exit(exit_code), message: text, metrics }
}

/// Une ligne de série : son nom de famille, la série telle qu'écrite, et sa
/// valeur. `None` pour une ligne illisible ou une valeur non finie.
fn prometheus_line(line: &str) -> Option<(&str, String, f64)> {
    let name_end = line
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
        .unwrap_or(line.len());
    let family = &line[..name_end];
    if family.is_empty() || family.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let mut rest = &line[name_end..];
    let mut series = family.to_string();
    if rest.starts_with('{') {
        let close = labels_end(rest)?;
        series.push_str(&rest[..=close]);
        rest = &rest[close + 1..];
    }
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let raw = rest.split_whitespace().next()?;
    let value = raw.parse::<f64>().ok().filter(|v| v.is_finite())?;
    if series.chars().count() > MAX_METRIC_NAME {
        return None;
    }
    Some((family, series, value))
}

/// Position de l'accolade qui ferme les étiquettes, guillemets et
/// échappements compris.
fn labels_end(text: &str) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '}' if !quoted => return Some(i),
            _ => {}
        }
    }
    None
}
