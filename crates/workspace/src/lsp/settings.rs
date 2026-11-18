//! `rustode.languageServers` in `settings.json`: which language server to
//! start for each language id. VS Code has no such setting (its language
//! clients live in extensions), so the key is rustode's own:
//!
//! ```jsonc
//! "rustode.languageServers": {
//!   "rust": { "command": "rust-analyzer", "args": [], "initializationOptions": {} }
//! }
//! ```
//!
//! `command` is required; `args` (strings) and `initializationOptions` (any
//! JSON, passed in `initialize`) are optional. An entry with a problem is
//! reported and, when it cannot be used, skipped; the other entries still
//! work.

use rustode_config::Value;
use std::collections::BTreeMap;

/// The setting's id.
pub(crate) const KEY: &str = "rustode.languageServers";

/// How to start one language server.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub initialization_options: Option<serde_json::Value>,
}

/// The usable entries by language id, and a sentence for each problem.
pub(crate) fn parse(value: Option<&Value>) -> (BTreeMap<String, ServerConfig>, Vec<String>) {
    let mut configs = BTreeMap::new();
    let mut problems = Vec::new();
    let Some(Value::Object(entries)) = value else {
        return (configs, problems);
    };
    for (language, entry) in entries {
        let mut report = |what: String| problems.push(format!("{KEY}[\"{language}\"]: {what}"));
        if let Some(config) = parse_entry(entry, &mut report) {
            configs.insert(language.clone(), config);
        }
    }
    (configs, problems)
}

/// One entry, or `None` when it cannot be used; problems go to `report`.
fn parse_entry(entry: &Value, report: &mut impl FnMut(String)) -> Option<ServerConfig> {
    let Value::Object(fields) = entry else {
        report(format!("must be an object, not {}", entry.type_name()));
        return None;
    };
    for key in fields.keys() {
        if !matches!(key.as_str(), "command" | "args" | "initializationOptions") {
            report(format!("unknown property \"{key}\""));
        }
    }
    let command = match fields.get("command") {
        Some(Value::String(command)) if !command.is_empty() => command.clone(),
        Some(Value::String(_)) => {
            report("\"command\" is empty".to_owned());
            return None;
        }
        Some(other) => {
            report(format!(
                "\"command\" must be a string, not {}",
                other.type_name()
            ));
            return None;
        }
        None => {
            report("\"command\" is missing".to_owned());
            return None;
        }
    };
    let args = match fields.get("args") {
        None => Vec::new(),
        Some(Value::Array(items)) => {
            let strings: Vec<String> = items
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect();
            if strings.len() != items.len() {
                report("\"args\" must be an array of strings".to_owned());
                return None;
            }
            strings
        }
        Some(_) => {
            report("\"args\" must be an array of strings".to_owned());
            return None;
        }
    };
    Some(ServerConfig {
        command,
        args,
        initialization_options: fields.get("initializationOptions").map(to_json),
    })
}

/// A settings value as JSON; whole numbers stay integers.
pub(crate) fn to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Number(n) => whole(*n).map_or_else(
            || {
                serde_json::Number::from_f64(*n)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number)
            },
            |i| serde_json::Value::Number(i.into()),
        ),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Array(items) => serde_json::Value::Array(items.iter().map(to_json).collect()),
        Value::Object(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), to_json(value)))
                .collect(),
        ),
    }
}

/// `n` as an integer when it is a whole number that fits.
fn whole(n: f64) -> Option<i64> {
    const LIMIT: f64 = 9_007_199_254_740_992.0; // 2^53: every integer below is exact
    if n.fract() != 0.0 || n.abs() > LIMIT {
        return None;
    }
    format!("{n:.0}").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_keep_their_kind_in_initialization_options() {
        let value = Value::Array(vec![
            Value::Number(2.0),
            Value::Number(-1.5),
            Value::Number(1e300),
            Value::Null,
            Value::Bool(true),
        ]);
        assert_eq!(
            to_json(&value),
            serde_json::json!([2, -1.5, 1e300, null, true])
        );
    }

    #[test]
    fn entries_that_are_not_objects_or_have_an_empty_command_are_skipped() {
        let mut entries = BTreeMap::new();
        entries.insert("a".to_owned(), Value::Number(1.0));
        let mut empty = BTreeMap::new();
        empty.insert("command".to_owned(), Value::String(String::new()));
        entries.insert("b".to_owned(), Value::Object(empty));
        let (configs, problems) = parse(Some(&Value::Object(entries)));
        assert!(configs.is_empty());
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(parse(None).0.is_empty());
    }
}
