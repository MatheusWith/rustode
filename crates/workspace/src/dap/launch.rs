//! `launch.json`, in VS Code's format: JSON with comments and trailing
//! commas, at `.vscode/launch.json` of the opened folder
//! (`src/vs/workbench/contrib/debug/browser/debugConfigurationManager.ts:646`),
//! with a `configurations` array whose entries have a `name`, a `type` (which
//! debug adapter) and a `request` (`launch` or `attach`). Every other
//! property is the adapter's own and is passed to it as the arguments of the
//! `launch` request, as VS Code does. Problems are collected, never fatal: a
//! bad entry is skipped and the rest still work.
//!
//! Of VS Code's variables only `${workspaceFolder}` and
//! `${workspaceFolderBasename}` are substituted so far.

use rustode_config::jsonc::{self, Node, NodeKind};
use serde_json::{Map, Value};
use std::path::Path;

/// Strings and nesting deeper than this are not substituted into.
const MAX_DEPTH: usize = 64;

/// One configuration of `launch.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchConfiguration {
    /// The `name` shown to the user.
    pub name: String,
    /// The `type`: which debug adapter runs it.
    pub adapter_type: String,
    /// The `request`: `launch` or `attach`.
    pub request: String,
    /// The whole entry, as the adapter gets it (variables not substituted).
    pub(crate) body: Value,
}

/// What `launch.json` holds and what is wrong with it.
#[derive(Debug, Default)]
pub(crate) struct Launch {
    pub configurations: Vec<LaunchConfiguration>,
    pub problems: Vec<String>,
}

/// Reads the file; a missing one is no configurations and no problem.
pub(crate) fn load(path: &Path) -> Launch {
    match std::fs::read(path) {
        Ok(bytes) => parse(&String::from_utf8_lossy(&bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Launch::default(),
        Err(err) => Launch {
            configurations: Vec::new(),
            problems: vec![format!("cannot read launch.json: {err}")],
        },
    }
}

/// The configurations in `text`, and a sentence for each problem.
pub(crate) fn parse(text: &str) -> Launch {
    let parsed = jsonc::parse(text);
    let mut launch = Launch {
        configurations: Vec::new(),
        problems: parsed
            .errors
            .iter()
            .map(|e| format!("launch.json:{}: {}", line_of(text, e.span.start), e.error))
            .collect(),
    };
    let Some(root) = parsed.root else {
        return launch;
    };
    let NodeKind::Object(properties) = &root.kind else {
        launch
            .problems
            .push("launch.json: the top level must be an object".to_owned());
        return launch;
    };
    let configurations = properties
        .iter()
        .rev()
        .find(|p| p.key == "configurations")
        .and_then(|p| p.value.as_ref());
    match configurations.map(|node| &node.kind) {
        Some(NodeKind::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                match entry(item) {
                    Ok(configuration) => launch.configurations.push(configuration),
                    Err(why) => launch
                        .problems
                        .push(format!("launch.json: configurations[{index}]: {why}")),
                }
            }
        }
        Some(_) => launch
            .problems
            .push("launch.json: \"configurations\" must be an array".to_owned()),
        None => {}
    }
    launch
}

fn line_of(text: &str, offset: usize) -> String {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    format!("{line}:{column}")
}

fn entry(node: &Node) -> Result<LaunchConfiguration, String> {
    let NodeKind::Object(_) = &node.kind else {
        return Err("must be an object".to_owned());
    };
    let body = to_json(&node.to_value());
    let field = |key: &str| -> Result<String, String> {
        match body.get(key) {
            Some(Value::String(text)) if !text.is_empty() => Ok(text.clone()),
            Some(Value::String(_)) => Err(format!("\"{key}\" is empty")),
            Some(_) => Err(format!("\"{key}\" must be a string")),
            None => Err(format!("\"{key}\" is missing")),
        }
    };
    Ok(LaunchConfiguration {
        name: field("name")?,
        adapter_type: field("type")?,
        request: field("request")?,
        body,
    })
}

fn to_json(value: &rustode_config::Value) -> Value {
    use rustode_config::Value as V;
    match value {
        V::Null => Value::Null,
        V::Bool(b) => Value::Bool(*b),
        V::Number(n) if n.fract() == 0.0 && n.abs() < 9e15 => {
            #[allow(clippy::cast_possible_truncation)] // checked whole and small just above
            Value::from(*n as i64)
        }
        V::Number(n) => serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number),
        V::String(s) => Value::String(s.clone()),
        V::Array(items) => Value::Array(items.iter().map(to_json).collect()),
        V::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), to_json(value)))
                .collect::<Map<_, _>>(),
        ),
    }
}

/// `body` with `${workspaceFolder}` and `${workspaceFolderBasename}`
/// replaced in every string.
pub(crate) fn substitute(body: &Value, folder: &Path) -> Value {
    let root = folder.display().to_string();
    let base = folder
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    walk(body, &root, &base, 0)
}

fn walk(value: &Value, root: &str, base: &str, depth: usize) -> Value {
    if depth > MAX_DEPTH {
        return value.clone();
    }
    match value {
        Value::String(text) => Value::String(
            text.replace("${workspaceFolderBasename}", base)
                .replace("${workspaceFolder}", root),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| walk(v, root, base, depth + 1))
                .collect(),
        ),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), walk(v, root, base, depth + 1)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_trailing_commas_and_bad_entries() {
        let launch = parse(
            r#"{
              // VS Code's own file has comments
              "version": "0.2.0",
              "configurations": [
                { "name": "Run", "type": "test", "request": "launch", "program": "${workspaceFolder}/a", },
                { "name": "No type", "request": "launch" },
                7,
              ],
            }"#,
        );
        assert_eq!(launch.configurations.len(), 1);
        assert_eq!(launch.configurations[0].name, "Run");
        assert_eq!(launch.problems.len(), 2, "{:?}", launch.problems);
        let body = substitute(&launch.configurations[0].body, Path::new("/w/proj"));
        assert_eq!(body["program"], "/w/proj/a");
    }

    #[test]
    fn broken_files_are_problems() {
        assert_eq!(parse("").problems.len(), 0);
        assert_ne!(parse("{ \"configurations\": [ {").problems.len(), 0);
        assert_eq!(parse("[1]").problems.len(), 1);
        assert_eq!(parse(r#"{"configurations": 3}"#).problems.len(), 1);
    }
}
