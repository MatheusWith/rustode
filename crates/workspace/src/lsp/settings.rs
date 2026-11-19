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
//! work. An entry set to `null` switches that language's server off, built-in
//! default included.
//!
//! Built-in defaults ([`DEFAULTS`]): once a frontend asks for them
//! ([`crate::Workspace::use_default_language_servers`]), `rust-analyzer` serves
//! Rust and `typescript-language-server --stdio` serves TypeScript and
//! JavaScript when the program is on the search path and the user's settings
//! have no entry for that language. The user's entry always wins, whatever it
//! holds.

use rustode_config::Value;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::PathBuf;

/// The setting's id.
pub(crate) const KEY: &str = "rustode.languageServers";

/// How to start one language server.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub initialization_options: Option<serde_json::Value>,
}

/// The servers rustode starts without settings: language ids, program and
/// arguments.
pub(crate) const DEFAULTS: &[(&str, &str, &[&str])] = &[
    ("rust", "rust-analyzer", &[]),
    ("typescript", "typescript-language-server", &["--stdio"]),
    (
        "typescriptreact",
        "typescript-language-server",
        &["--stdio"],
    ),
    ("javascript", "typescript-language-server", &["--stdio"]),
    (
        "javascriptreact",
        "typescript-language-server",
        &["--stdio"],
    ),
];

/// Adds the built-in default of every language the user's settings do not
/// mention, when its program is an executable file in a folder of
/// `search_path` (a `PATH`-style list).
pub(crate) fn add_defaults(
    configs: &mut BTreeMap<String, ServerConfig>,
    value: Option<&Value>,
    search_path: &OsStr,
) {
    for &(language, command, args) in DEFAULTS {
        let mentioned =
            matches!(value, Some(Value::Object(entries)) if entries.contains_key(language));
        if mentioned {
            continue;
        }
        let Some(program) = on_path(command, search_path) else {
            continue;
        };
        configs.insert(
            language.to_owned(),
            ServerConfig {
                command: program.display().to_string(),
                args: args.iter().map(|a| (*a).to_owned()).collect(),
                initialization_options: None,
            },
        );
    }
}

/// The executable file named `command` in the first folder of `search_path`
/// that has one. The server is started from this path, so the search path
/// given is the one that decides, not the process's own.
fn on_path(command: &str, search_path: &OsStr) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    std::env::split_paths(search_path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(command))
        .find(|file| {
            file.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// The usable entries by language id, and a sentence for each problem.
pub(crate) fn parse(value: Option<&Value>) -> (BTreeMap<String, ServerConfig>, Vec<String>) {
    let mut configs = BTreeMap::new();
    let mut problems = Vec::new();
    let Some(Value::Object(entries)) = value else {
        return (configs, problems);
    };
    for (language, entry) in entries {
        if matches!(entry, Value::Null) {
            continue; // switched off on purpose
        }
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
    use std::path::Path;

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
    fn defaults_start_from_the_path_found_and_skip_what_the_user_mentions() {
        let dir =
            std::env::temp_dir().join(format!("rustode-defaults-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let program = dir.join("typescript-language-server");
        std::fs::write(&program, "#!/bin/sh\n").expect("file");
        let mut mode = std::fs::metadata(&program).expect("meta").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
        std::fs::set_permissions(&program, mode).expect("chmod");
        let path = std::env::join_paths([&dir, Path::new("relative/bin")]).expect("path");

        let mut configs = BTreeMap::new();
        add_defaults(&mut configs, None, &path);
        let ts = &configs["typescript"];
        assert_eq!(ts.command, program.display().to_string());
        assert_eq!(ts.args, ["--stdio"]);
        assert!(!configs.contains_key("rust"), "rust-analyzer is not there");

        let mut mentioned = BTreeMap::new();
        mentioned.insert("javascript".to_owned(), Value::Null);
        let mut configs = BTreeMap::new();
        add_defaults(&mut configs, Some(&Value::Object(mentioned)), &path);
        assert!(!configs.contains_key("javascript"), "null switches it off");
        assert!(configs.contains_key("typescriptreact"));
        let _ = std::fs::remove_dir_all(&dir);
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
        let mut off = BTreeMap::new();
        off.insert("c".to_owned(), Value::Null);
        let (configs, problems) = parse(Some(&Value::Object(off)));
        assert!(configs.is_empty() && problems.is_empty());
    }
}
