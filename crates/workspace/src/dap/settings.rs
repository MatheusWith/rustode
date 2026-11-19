//! `rustode.debugAdapters` in `settings.json`: which debug adapter to start
//! for each launch configuration `type`. VS Code has no such setting: its
//! debuggers come from extensions, which contribute a `type` and the program
//! that speaks DAP (`src/vs/workbench/contrib/debug/common/debugger.ts`), and
//! rustode has no extension host yet, so the key is rustode's own, in the
//! shape of `rustode.languageServers`:
//!
//! ```jsonc
//! "rustode.debugAdapters": {
//!   "lldb": { "command": "lldb-dap", "args": [] }
//! }
//! ```
//!
//! `command` is required; `args` (strings) and `env` (an object of strings
//! added to the adapter's environment) are optional. An entry with a
//! problem is reported and, when it cannot be used, skipped; the other
//! entries still work. An entry set to `null` switches that type's adapter
//! off, which is how a user turns off a built-in one as for
//! `rustode.languageServers`; rustode has no built-in debug adapter, so
//! today the user's entries are all there is and `null` just leaves the
//! type without one. The setting is read again whenever the settings
//! change (`Workspace::dap_config_changed`); a running session keeps the
//! adapter it started with and the new value is used at the next start.

use rustode_config::Value;
use std::collections::BTreeMap;

/// The setting's id.
pub(crate) const KEY: &str = "rustode.debugAdapters";

/// How to start one debug adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdapterConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The usable entries by configuration type, and a sentence for each problem.
pub(crate) fn parse(value: Option<&Value>) -> (BTreeMap<String, AdapterConfig>, Vec<String>) {
    let mut adapters = BTreeMap::new();
    let mut problems = Vec::new();
    let Some(Value::Object(entries)) = value else {
        return (adapters, problems);
    };
    for (kind, entry) in entries {
        if matches!(entry, Value::Null) {
            continue;
        }
        let mut report = |what: String| problems.push(format!("{KEY}[\"{kind}\"]: {what}"));
        if let Some(config) = parse_entry(entry, &mut report) {
            adapters.insert(kind.clone(), config);
        }
    }
    (adapters, problems)
}

fn parse_entry(entry: &Value, report: &mut impl FnMut(String)) -> Option<AdapterConfig> {
    let Value::Object(fields) = entry else {
        report(format!("must be an object, not {}", entry.type_name()));
        return None;
    };
    for key in fields.keys() {
        if !matches!(key.as_str(), "command" | "args" | "env") {
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
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(arg) => Some(arg.clone()),
                other => {
                    report(format!(
                        "\"args\" holds {}, not a string; skipped",
                        other.type_name()
                    ));
                    None
                }
            })
            .collect(),
        Some(other) => {
            report(format!(
                "\"args\" must be an array, not {}; ignored",
                other.type_name()
            ));
            Vec::new()
        }
    };
    let env = parse_env(fields.get("env"), report);
    Some(AdapterConfig { command, args, env })
}

/// The `env` entries that are strings, by name; the rest are reported.
fn parse_env(value: Option<&Value>, report: &mut impl FnMut(String)) -> Vec<(String, String)> {
    match value {
        None => Vec::new(),
        Some(Value::Object(vars)) => vars
            .iter()
            .filter(|(name, _)| !name.is_empty() && !name.contains(['=', '\0']))
            .filter_map(|(name, var)| match var {
                Value::String(text) if !text.contains('\0') => Some((name.clone(), text.clone())),
                other => {
                    report(format!(
                        "\"env\" entry \"{name}\" must be a string, not {}; skipped",
                        other.type_name()
                    ));
                    None
                }
            })
            .collect(),
        Some(other) => {
            report(format!(
                "\"env\" must be an object, not {}; ignored",
                other.type_name()
            ));
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> (BTreeMap<String, AdapterConfig>, Vec<String>) {
        let root = rustode_config::jsonc::parse(text)
            .root
            .map(|n| n.to_value());
        parse(root.as_ref())
    }

    #[test]
    fn entries_are_checked_one_by_one() {
        let (adapters, problems) = parsed(
            r#"{ "ok": {"command": "x", "args": ["-a", 3]},
                 "no": {}, "bad": 4, "odd": {"command": "y", "extra": 1} }"#,
        );
        assert_eq!(adapters["ok"].args, ["-a"]);
        assert_eq!(adapters["odd"].command, "y");
        assert!(!adapters.contains_key("no") && !adapters.contains_key("bad"));
        assert_eq!(problems.len(), 4, "{problems:?}");
    }

    #[test]
    fn a_setting_that_is_not_an_object_is_nothing() {
        assert_eq!(parsed("[1]").0.len(), 0);
        assert_eq!(parse(None).1.len(), 0);
    }
}
