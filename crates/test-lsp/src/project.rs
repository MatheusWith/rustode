//! The test server's project-wide features: definition, references, rename
//! and document formatting. They work on the `.rs` files under the
//! workspace root (the open documents as they are in memory, the others as
//! they are on disk), and on a naive rule: a declaration is a word right
//! after `fn ` or `let `, and every whole-word match of a name is a use.

use super::{Server, is_word, lines, units, word_around};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// The most files read from disk, and the deepest folder walked.
const MAX_FILES: usize = 200;
const MAX_DEPTH: usize = 4;

/// One whole-word match of a name.
struct Hit {
    line: usize,
    start: usize,
    end: usize,
    declaration: bool,
}

/// The whole-word matches of `word` in `text`, in `encoding`'s units.
fn hits(text: &str, word: &str, encoding: &str) -> Vec<Hit> {
    let mut found = Vec::new();
    for (number, (_, line)) in lines(text).into_iter().enumerate() {
        for (byte, _) in line.match_indices(word) {
            let before = line[..byte].chars().next_back();
            let after = line[byte + word.len()..].chars().next();
            if before.is_some_and(is_word) || after.is_some_and(is_word) {
                continue;
            }
            let start: usize = line[..byte].chars().map(|c| units(c, encoding)).sum();
            let width: usize = word.chars().map(|c| units(c, encoding)).sum();
            found.push(Hit {
                line: number,
                start,
                end: start + width,
                declaration: line[..byte].ends_with("fn ") || line[..byte].ends_with("let "),
            });
        }
    }
    found
}

fn range(hit: &Hit) -> Value {
    json!({
        "start": {"line": hit.line, "character": hit.start},
        "end": {"line": hit.line, "character": hit.end},
    })
}

fn walk(dir: &Path, depth: usize, files: &mut BTreeMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        if name.is_none_or(|n| n.starts_with('.') || n == "target" || n == "user") {
            continue;
        }
        if path.is_dir() && depth < MAX_DEPTH {
            walk(&path, depth + 1, files);
        } else if path.extension().is_some_and(|e| e == "rs")
            && files.len() < MAX_FILES
            && std::fs::metadata(&path).is_ok_and(|m| m.len() <= 64 * 1024)
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            files.insert(format!("file://{}", path.display()), text);
        }
    }
}

impl Server {
    /// Every `.rs` file of the project by URI: the open documents win over
    /// the disk.
    fn project(&self) -> BTreeMap<String, String> {
        let mut files = BTreeMap::new();
        if let Some(root) = &self.root {
            walk(Path::new(root), 0, &mut files);
        }
        for (uri, text) in &self.documents {
            files.insert(uri.clone(), text.clone());
        }
        files
    }

    /// The word at the position of `params`.
    fn word(&self, params: &Value) -> Option<String> {
        let (line, _, column) = self.locate(params)?;
        let (start, end) = word_around(line, column)?;
        Some(line[start..end].to_owned())
    }

    /// `textDocument/definition`: the declarations of the word; one is a
    /// `Location` (or a `LocationLink` with `--definition-links`), several a
    /// list, none `null`.
    pub(super) fn definition(&self, params: &Value) -> Value {
        let Some(word) = self.word(params) else {
            return Value::Null;
        };
        let mut found = Vec::new();
        for (uri, text) in self.project() {
            for hit in hits(&text, &word, &self.encoding)
                .iter()
                .filter(|h| h.declaration)
            {
                found.push(if self.options.definition_links {
                    json!({
                        "targetUri": uri,
                        "targetRange": range(hit),
                        "targetSelectionRange": range(hit),
                    })
                } else {
                    json!({"uri": uri, "range": range(hit)})
                });
            }
        }
        match found.len() {
            0 => Value::Null,
            1 if !self.options.definition_links => found.remove(0),
            _ => Value::Array(found),
        }
    }

    /// `textDocument/references`: every whole-word match, declarations
    /// included.
    pub(super) fn references(&self, params: &Value) -> Value {
        let Some(word) = self.word(params) else {
            return Value::Null;
        };
        let mut found = Vec::new();
        for (uri, text) in self.project() {
            for hit in hits(&text, &word, &self.encoding) {
                found.push(json!({"uri": uri, "range": range(&hit)}));
            }
        }
        Value::Array(found)
    }

    /// `textDocument/rename`: edits for every whole-word match. A name that
    /// is not a word is refused with -32602. `--rename MODE` changes the
    /// answer: `changes` (the old `changes` shape), `stale` (versions that
    /// are not the documents'), `fileop` (a file creation), `hostile` (an
    /// edit past the end of the document), `overlap` (two edits on the same
    /// text) or `null`.
    pub(super) fn rename(&self, params: &Value) -> Result<Value, Value> {
        let name = params["newName"].as_str().unwrap_or_default();
        if name.is_empty() || !name.chars().all(is_word) {
            return Err(
                json!({"code": -32602, "message": format!("'{name}' is not a valid name")}),
            );
        }
        let Some(word) = self.word(params) else {
            return Ok(Value::Null);
        };
        let mode = self.options.rename.as_deref().unwrap_or_default();
        if mode == "null" {
            return Ok(Value::Null);
        }
        let mut per_file: Vec<(String, Vec<Value>)> = Vec::new();
        for (uri, text) in self.project() {
            let edits: Vec<Value> = hits(&text, &word, &self.encoding)
                .iter()
                .map(|hit| json!({"range": range(hit), "newText": name}))
                .collect();
            if !edits.is_empty() {
                per_file.push((uri, edits));
            }
        }
        match mode {
            "changes" => {
                let changes: serde_json::Map<String, Value> = per_file
                    .into_iter()
                    .map(|(uri, edits)| (uri, Value::Array(edits)))
                    .collect();
                return Ok(json!({"changes": changes}));
            }
            "hostile" => {
                if let Some((_, edits)) = per_file.first_mut() {
                    let far = json!({"line": 99_999, "character": 99_999});
                    edits.push(json!({"range": {"start": far, "end": far}, "newText": "X"}));
                }
            }
            "overlap" => {
                if let Some((_, edits)) = per_file.first_mut()
                    && let Some(first) = edits.first().cloned()
                {
                    edits.push(first);
                }
            }
            _ => {}
        }
        let skew = i64::from(mode == "stale") * 100;
        let mut changes: Vec<Value> = per_file
            .into_iter()
            .map(|(uri, edits)| {
                let version = self
                    .versions
                    .get(&uri)
                    .map_or(Value::Null, |v| json!(v + skew));
                json!({"textDocument": {"uri": uri, "version": version}, "edits": edits})
            })
            .collect();
        if mode == "fileop" {
            changes.push(json!({"kind": "create", "uri": "file:///nowhere/new.rs"}));
        }
        Ok(json!({"documentChanges": changes}))
    }

    /// `textDocument/formatting`: deletes the trailing spaces and tabs of
    /// every line. `--hostile-format` adds an edit past the end of the
    /// document.
    pub(super) fn formatting(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let Some(text) = self.documents.get(uri) else {
            return Value::Null;
        };
        let mut edits = Vec::new();
        for (number, (_, line)) in lines(text).into_iter().enumerate() {
            let kept = line.trim_end_matches([' ', '\t']);
            if kept.len() != line.len() {
                let from: usize = kept.chars().map(|c| units(c, &self.encoding)).sum();
                let to: usize = line.chars().map(|c| units(c, &self.encoding)).sum();
                edits.push(json!({
                    "range": {
                        "start": {"line": number, "character": from},
                        "end": {"line": number, "character": to},
                    },
                    "newText": "",
                }));
            }
        }
        if self.options.hostile_format {
            let far = json!({"line": 99_999, "character": 99_999});
            edits.push(json!({"range": {"start": far, "end": far}, "newText": "// end\n"}));
        }
        Value::Array(edits)
    }
}
