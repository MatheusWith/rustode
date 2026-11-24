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
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

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
    pub problems: Vec<LaunchProblem>,
}

/// What is wrong with an entry of `configurations`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EntryProblem {
    NotAnObject,
    Empty(&'static str),
    NotAString(&'static str),
    Missing(&'static str),
}

impl fmt::Display for EntryProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject => f.write_str("it must be an object"),
            Self::Empty(key) => write!(f, "\"{key}\" is empty"),
            Self::NotAString(key) => write!(f, "\"{key}\" must be a string"),
            Self::Missing(key) => write!(f, "\"{key}\" is missing"),
        }
    }
}

/// One problem of `launch.json`. Every variant names the file; the entry
/// that is skipped is named by its position.
#[derive(Debug)]
pub(crate) enum LaunchProblem {
    /// The file exists but cannot be read.
    Unreadable { path: PathBuf, source: io::Error },
    /// A syntax error, with its line.
    Syntax {
        path: PathBuf,
        line: String,
        message: String,
    },
    /// The top level is not an object.
    NotAnObject(PathBuf),
    /// `configurations` is not an array.
    ConfigurationsNotAnArray(PathBuf),
    /// An entry of `configurations` is skipped.
    Entry {
        path: PathBuf,
        index: usize,
        problem: EntryProblem,
    },
}

impl fmt::Display for LaunchProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            Self::Syntax {
                path,
                line,
                message,
            } => write!(f, "{}:{line}: {message}", path.display()),
            Self::NotAnObject(path) => {
                write!(f, "{}: the top level must be an object", path.display())
            }
            Self::ConfigurationsNotAnArray(path) => {
                write!(f, "{}: \"configurations\" must be an array", path.display())
            }
            Self::Entry {
                path,
                index,
                problem,
            } => write!(
                f,
                "{}: configurations[{index}] is skipped: {problem}",
                path.display()
            ),
        }
    }
}

/// Reads the file; a missing one is no configurations and no problem.
pub(crate) fn load(path: &Path) -> Launch {
    match std::fs::read(path) {
        Ok(bytes) => parse(path, &String::from_utf8_lossy(&bytes)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Launch::default(),
        Err(source) => Launch {
            configurations: Vec::new(),
            problems: vec![LaunchProblem::Unreadable {
                path: path.to_owned(),
                source,
            }],
        },
    }
}

/// The configurations in `text` (read from `path`), and a problem for each
/// thing wrong with it.
pub(crate) fn parse(path: &Path, text: &str) -> Launch {
    let parsed = jsonc::parse(text);
    let mut launch = Launch {
        configurations: Vec::new(),
        problems: parsed
            .errors
            .iter()
            .map(|e| LaunchProblem::Syntax {
                path: path.to_owned(),
                line: line_of(text, e.span.start),
                message: e.error.to_string(),
            })
            .collect(),
    };
    let Some(root) = parsed.root else {
        return launch;
    };
    let NodeKind::Object(properties) = &root.kind else {
        launch
            .problems
            .push(LaunchProblem::NotAnObject(path.to_owned()));
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
                    Err(problem) => launch.problems.push(LaunchProblem::Entry {
                        path: path.to_owned(),
                        index,
                        problem,
                    }),
                }
            }
        }
        Some(_) => launch
            .problems
            .push(LaunchProblem::ConfigurationsNotAnArray(path.to_owned())),
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

fn entry(node: &Node) -> Result<LaunchConfiguration, EntryProblem> {
    let NodeKind::Object(_) = &node.kind else {
        return Err(EntryProblem::NotAnObject);
    };
    let body = to_json(&node.to_value());
    let field = |key: &'static str| -> Result<String, EntryProblem> {
        match body.get(key) {
            Some(Value::String(text)) if !text.is_empty() => Ok(text.clone()),
            Some(Value::String(_)) => Err(EntryProblem::Empty(key)),
            Some(_) => Err(EntryProblem::NotAString(key)),
            None => Err(EntryProblem::Missing(key)),
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
            Path::new("launch.json"),
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
        assert_eq!(parse(Path::new("l"), "").problems.len(), 0);
        assert_ne!(
            parse(Path::new("l"), "{ \"configurations\": [ {")
                .problems
                .len(),
            0
        );
        assert_eq!(parse(Path::new("l"), "[1]").problems.len(), 1);
        assert_eq!(
            parse(Path::new("l"), r#"{"configurations": 3}"#)
                .problems
                .len(),
            1
        );
    }
}
