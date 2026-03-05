use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub struct RenderResult {
    pub output: String,
    pub unresolved: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn render_template_preview(template: &str, vars_json: &str) -> RenderResult {
    let mut warnings = Vec::new();
    let vars = parse_vars(vars_json, &mut warnings);

    // Remove script/control blocks from preview output (they are runtime-driven in diaLIMS).
    let mut out = template.to_string();
    out = regex_replace(&out, r"(?s)\{!.*?!\}", "\n[run-first script omitted]\n");
    out = regex_replace(&out, r"\{\|:\s*[^}]*\}", "");
    out = regex_replace(&out, r"\{\|\|:\s*[^}]*\}", "");
    out = regex_replace(&out, r"\{:\|\}", "");
    out = regex_replace(&out, r"\{:\|\|\}", "");
    out = regex_replace(&out, r"\{/[^\n}]*\}", "");

    let placeholder_re = Regex::new(r"\{([A-Za-z_][A-Za-z0-9_\.]*)\}").expect("valid regex");
    let mut unresolved = BTreeSet::new();
    let rendered = placeholder_re
        .replace_all(&out, |caps: &regex::Captures<'_>| {
            let key = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            if let Some(v) = vars.get(key) {
                v.clone()
            } else {
                unresolved.insert(key.to_string());
                format!("{{MISSING:{key}}}")
            }
        })
        .to_string();

    RenderResult {
        output: rendered,
        unresolved: unresolved.into_iter().collect(),
        warnings,
    }
}

pub fn placeholders_as_sample_json(template: &str) -> String {
    let re = Regex::new(r"\{([A-Za-z_][A-Za-z0-9_\.]*)\}").expect("valid regex");
    let mut map = BTreeMap::<String, String>::new();
    for caps in re.captures_iter(template) {
        let key = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        map.entry(key.to_string())
            .or_insert_with(|| format!("sample_{key}"));
    }
    serde_json::to_string_pretty(&map).unwrap_or_else(|_| "{}".to_string())
}

fn parse_vars(input: &str, warnings: &mut Vec<String>) -> BTreeMap<String, String> {
    if input.trim().is_empty() {
        return BTreeMap::new();
    }

    let value: serde_json::Value = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(err) => {
            warnings.push(format!("Vars JSON parse error: {err}"));
            return BTreeMap::new();
        }
    };

    match value {
        serde_json::Value::Object(obj) => obj
            .into_iter()
            .map(|(k, v)| (k, value_to_text(v)))
            .collect::<BTreeMap<_, _>>(),
        _ => {
            warnings.push("Vars JSON must be a JSON object {\"key\":\"value\"}".to_string());
            BTreeMap::new()
        }
    }
}

fn value_to_text(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(v) => v.to_string(),
        serde_json::Value::Number(v) => v.to_string(),
        serde_json::Value::String(v) => v,
        other => other.to_string(),
    }
}

fn regex_replace(input: &str, pattern: &str, replacement: &str) -> String {
    Regex::new(pattern)
        .expect("valid regex")
        .replace_all(input, replacement)
        .to_string()
}
