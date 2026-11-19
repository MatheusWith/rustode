//! Rename and format: requests whose answers are edits the workspace
//! applies, to the open documents and to the files of the project.
//!
//! Studied in VS Code and reimplemented, see `ATTRIBUTION.md`:
//!
//! - **Rename** (`editor.action.rename`, F2,
//!   `src/vs/editor/contrib/rename/browser/rename.ts:365-370`) asks for the
//!   new name starting from the word under the cursor, which is also what
//!   it falls back to when no provider resolves the location
//!   (`rename.ts:60-110`; `prepareRename` is not asked yet). The server's
//!   `WorkspaceEdit` reaches every file it names.
//! - **Format Document** (`editor.action.formatDocument`, Ctrl+Shift+I on
//!   Linux, `src/vs/editor/contrib/format/browser/formatActions.ts:218-224`)
//!   asks the server for the edits that format the whole document and applies
//!   them (`src/vs/editor/contrib/format/browser/format.ts:310-345`).
//! - **Bulk edits** abort when a document's version is not the one an edit
//!   was computed for
//!   (`src/vs/workbench/contrib/bulkEdit/browser/bulkTextEdits.ts:216-218`),
//!   and a file that is not open opens (as an unsaved change) to take its
//!   edits.
//!
//! An edit is applied or refused as a whole: every file is checked before
//! the first one changes. Each document's edits are one undo step of that
//! document, which leaves the cursor where the text around it moved. A
//! hostile `WorkspaceEdit` is bounded ([`MAX_FILES`], [`MAX_EDITS`]), its
//! positions are clamped to the text, and overlapping edits are refused (LSP
//! forbids them). File operations (create, rename, delete) are refused: they
//! are not supported yet.

use super::encoding::PositionEncoding;
use super::navigate::{RenameTarget, word_at};
use super::requests::{Kind, Op, Request, lsp_position};
use super::uri;
use crate::{DocumentId, Error, Range, Selection, Workspace};
use serde_json::{Value, json};
use std::path::PathBuf;

/// The most files one `WorkspaceEdit` may change.
pub const MAX_FILES: usize = 1_000;
/// The most edits one file may take from one `WorkspaceEdit`.
pub const MAX_EDITS: usize = 100_000;

/// One `TextEdit`, as the server sent it.
#[derive(Debug)]
struct TextEdit {
    start: (u64, u64),
    end: (u64, u64),
    text: String,
}

/// What a `WorkspaceEdit` does to one file.
#[derive(Debug)]
struct FileEdits {
    path: PathBuf,
    /// The version of the document the edits were computed for.
    version: Option<i64>,
    edits: Vec<TextEdit>,
}

fn pair(position: &Value) -> (u64, u64) {
    (
        position["line"].as_u64().unwrap_or(0),
        position["character"].as_u64().unwrap_or(0),
    )
}

/// The edits of a `TextEdit[]`; a malformed one refuses the lot.
fn text_edits(list: &Value) -> Result<Vec<TextEdit>, String> {
    let list = list.as_array().ok_or("the edits are not a list")?;
    if list.len() > MAX_EDITS {
        return Err(format!("more than {MAX_EDITS} edits in one file"));
    }
    list.iter()
        .map(|edit| {
            let range = &edit["range"];
            let text = edit["newText"].as_str();
            match (range.is_object(), text) {
                (true, Some(text)) => Ok(TextEdit {
                    start: pair(&range["start"]),
                    end: pair(&range["end"]),
                    text: text.to_owned(),
                }),
                _ => Err("an edit has no range or no new text".to_owned()),
            }
        })
        .collect()
}

/// The files a `WorkspaceEdit` changes, one entry per file.
fn workspace_edit(edit: &Value) -> Result<Vec<FileEdits>, String> {
    let mut files: Vec<FileEdits> = Vec::new();
    let mut add = |uri_text: &str, version: Option<i64>, edits: Vec<TextEdit>| {
        let path = uri::to_path(uri_text).ok_or_else(|| format!("{uri_text} is not a file"))?;
        if let Some(file) = files.iter_mut().find(|f| f.path == path) {
            file.edits.extend(edits);
        } else {
            files.push(FileEdits {
                path,
                version,
                edits,
            });
        }
        Ok::<(), String>(())
    };
    if let Some(changes) = edit["documentChanges"].as_array() {
        for change in changes {
            if change.get("kind").is_some() {
                return Err(
                    "creating, renaming and deleting files are not supported yet".to_owned(),
                );
            }
            let document = &change["textDocument"];
            let uri_text = document["uri"].as_str().ok_or("an edit names no file")?;
            add(
                uri_text,
                document["version"].as_i64(),
                text_edits(&change["edits"])?,
            )?;
        }
    } else if let Some(changes) = edit["changes"].as_object() {
        for (uri_text, edits) in changes {
            add(uri_text, None, text_edits(edits)?)?;
        }
    }
    if files.len() > MAX_FILES {
        return Err(format!("more than {MAX_FILES} files to change"));
    }
    Ok(files)
}

/// Where char `offset` goes when `edits` (sorted, not overlapping, as char
/// ranges and the chars that replace them) are applied: past an edit it
/// moves with the text, inside one it goes to the end of the new text.
fn follow(offset: usize, edits: &[(Range, usize)]) -> usize {
    let (mut added, mut removed) = (0_usize, 0_usize);
    for (range, inserted) in edits {
        if range.end <= offset {
            added = added.saturating_add(*inserted);
            removed = removed.saturating_add(range.len());
        } else if range.start < offset {
            return range
                .start
                .saturating_add(added)
                .saturating_sub(removed)
                .saturating_add(*inserted);
        } else {
            break;
        }
    }
    offset.saturating_add(added).saturating_sub(removed)
}

impl Workspace {
    /// What a rename would change: the word at the cursor, which the
    /// frontend offers as the starting text of the new name. `None` when the
    /// document has no running server that renames, or no word is there.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn rename_target(&self, id: DocumentId) -> Result<Option<RenameTarget>, Error> {
        let doc = self.document(id).ok_or(Error::UnknownDocument(id))?;
        if self.lsp_target(id, |f| f.rename).is_none() {
            return Ok(None);
        }
        Ok(word_at(doc, doc.cursor()).map(|(range, text)| RenameTarget { range, text }))
    }

    /// Asks the document's language server to rename the symbol at the
    /// cursor. The server's edit is applied when the answer arrives with
    /// [`poll_language_servers`](Self::poll_language_servers), and
    /// [`Event::Notice`] says what changed; each document it touches takes
    /// its edits as one undo step. Returns whether a request went out:
    /// `false` when there is no running server that renames, no word at the
    /// cursor, or `new_name` is empty, has a line break or is the word itself.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn rename_symbol(&mut self, id: DocumentId, new_name: &str) -> Result<bool, Error> {
        let doc = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let (version, cursor) = (doc.version(), doc.cursor());
        let Some((_, word)) = word_at(doc, cursor) else {
            return Ok(false);
        };
        let new_name = new_name.trim();
        if new_name.is_empty() || new_name == word || new_name.contains(['\r', '\n']) {
            return Ok(false);
        }
        let Some((server, uri, encoding)) = self.lsp_target(id, |f| f.rename) else {
            return Ok(false);
        };
        let Some(doc) = self.document(id) else {
            return Ok(false);
        };
        let params = json!({
            "textDocument": {"uri": uri},
            "position": lsp_position(doc, cursor, encoding),
            "newName": new_name,
        });
        let Some(request) = self.lsp_request(server, Kind::Rename, "textDocument/rename", &params)
        else {
            return Ok(false);
        };
        self.lsp.ops.insert(
            request,
            Op {
                document: id,
                version,
                word,
                name: new_name.to_owned(),
            },
        );
        Ok(true)
    }

    /// Asks the document's language server to format it. The edits are
    /// applied as one undo step when the answer arrives; [`Event::Notice`]
    /// says when there was nothing to change. Returns whether a request went
    /// out: `false` when there is no running server that formats.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn format_document(&mut self, id: DocumentId) -> Result<bool, Error> {
        let doc = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let version = doc.version();
        let Some((server, uri, _)) = self.lsp_target(id, |f| f.formatting) else {
            return Ok(false);
        };
        let params = json!({
            "textDocument": {"uri": uri},
            "options": {
                "tabSize": self.tab_size(id)?,
                "insertSpaces": self.insert_spaces(id)?,
            },
        });
        let Some(request) =
            self.lsp_request(server, Kind::Format, "textDocument/formatting", &params)
        else {
            return Ok(false);
        };
        self.lsp.ops.insert(
            request,
            Op {
                document: id,
                version,
                word: String::new(),
                name: String::new(),
            },
        );
        Ok(true)
    }

    /// A rename or format answer arrived.
    pub(super) fn edits_answered(
        &mut self,
        request: Request,
        kind: Kind,
        result: &Value,
        failure: Option<String>,
    ) {
        let Some(op) = self.lsp.ops.remove(&request) else {
            return;
        };
        let current = self
            .document(op.document)
            .is_some_and(|doc| doc.version() == op.version);
        let what = if kind == Kind::Rename {
            "rename"
        } else {
            "format"
        };
        if !current {
            self.notice(format!(
                "The document changed before the language server answered: the {what} was not applied"
            ));
            return;
        }
        if let Some(why) = failure {
            self.notice(format!("The {what} failed: {why}"));
            return;
        }
        let Some(encoding) = self.lsp.server(request.0).map(|s| s.capabilities.encoding) else {
            return;
        };
        let files = if kind == Kind::Rename {
            if result.is_null() {
                self.notice("The rename produced no edits".to_owned());
                return;
            }
            workspace_edit(result)
        } else {
            let Some(path) = self.document(op.document).map(|d| d.path().to_owned()) else {
                return;
            };
            let edits = if result.is_null() {
                Ok(Vec::new())
            } else {
                text_edits(result)
            };
            edits.map(|edits| {
                vec![FileEdits {
                    path,
                    version: None,
                    edits,
                }]
            })
        };
        let applied = files.and_then(|files| self.apply_edits(&files, encoding));
        match (applied, kind) {
            (Err(why), _) => self.notice(format!("The {what} was not applied: {why}")),
            (Ok((_, 0)), Kind::Format) => self.notice("Nothing to format".to_owned()),
            (Ok(_), Kind::Format) => self.notice("Formatted the document".to_owned()),
            (Ok((files, edits)), _) => self.notice(format!(
                "Renamed '{}' to '{}': {edits} edit(s) in {files} file(s)",
                op.word, op.name
            )),
        }
    }

    /// Applies `files` as a whole: every file is opened and checked before
    /// the first one changes. Returns how many files changed and how many
    /// edits they took.
    fn apply_edits(
        &mut self,
        files: &[FileEdits],
        encoding: PositionEncoding,
    ) -> Result<(usize, usize), String> {
        for file in files {
            let expected = file.version;
            let open = self.find_open(&file.path);
            if let (Some(expected), Some(id)) = (expected, open)
                && let Some(tracked) = self.lsp.tracked.get(&id)
                && tracked.version != expected
            {
                return Err(format!(
                    "{} changed since the edit was computed",
                    file.path.display()
                ));
            }
        }
        let active = self.active_document();
        let result = self.apply_opened(files, encoding);
        if let Some(active) = active {
            let _ = self.activate(active);
        }
        result
    }

    fn apply_opened(
        &mut self,
        files: &[FileEdits],
        encoding: PositionEncoding,
    ) -> Result<(usize, usize), String> {
        let mut planned = Vec::with_capacity(files.len());
        for file in files {
            let id = self.open(&file.path).map_err(|err| err.to_string())?;
            let doc = self
                .document(id)
                .ok_or_else(|| Error::UnknownDocument(id).to_string())?;
            let mut edits: Vec<(Range, String)> = file
                .edits
                .iter()
                .map(|edit| {
                    let start = super::offset_at(doc, edit.start.0, edit.start.1, encoding);
                    let end = super::offset_at(doc, edit.end.0, edit.end.1, encoding).max(start);
                    (Range::new(start, end), edit.text.clone())
                })
                .collect();
            edits.sort_by_key(|(range, _)| (range.start, range.end));
            if edits.windows(2).any(|w| w[1].0.start < w[0].0.end) {
                return Err(format!("{} has overlapping edits", file.path.display()));
            }
            let primary = doc.primary_selection();
            planned.push((id, edits, primary));
        }
        let mut changed = (0, 0);
        for (id, edits, primary) in planned {
            let mut sized = Vec::with_capacity(edits.len());
            let mut eol_edits = Vec::with_capacity(edits.len());
            for (range, text) in edits {
                let text = self
                    .with_document_eol(id, &text)
                    .map_err(|err| err.to_string())?;
                sized.push((range, text.chars().count()));
                eol_edits.push((range, text));
            }
            let after =
                Selection::new(follow(primary.anchor, &sized), follow(primary.head, &sized));
            let before = self.document(id).map(crate::Document::version);
            self.edit(id, |editor| editor.replace_ranges(eol_edits, after))
                .map_err(|err| err.to_string())?;
            if self.document(id).map(crate::Document::version) != before {
                changed.0 += 1;
                changed.1 += sized.len();
            }
        }
        Ok(changed)
    }

    /// `editor.insertSpaces` for the document's language: spaces unless the
    /// setting is `false`.
    fn insert_spaces(&self, id: DocumentId) -> Result<bool, Error> {
        let doc = self.document(id).ok_or(Error::UnknownDocument(id))?;
        Ok(!matches!(
            self.config
                .language_value(doc.language_id(), "editor.insertSpaces"),
            Some(crate::Value::Bool(false))
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_edit_is_read_in_both_shapes() {
        let edit = json!({"changes": {
            "file:///a.rs": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "newText": "x"}],
        }});
        let files = workspace_edit(&edit).expect("changes");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].edits[0].text, "x");

        let edit = json!({"documentChanges": [
            {"textDocument": {"uri": "file:///a.rs", "version": 3}, "edits": []},
            {"textDocument": {"uri": "file:///a.rs", "version": 3}, "edits": [
                {"range": {"start": {"line": 1, "character": 2}, "end": {"line": 1, "character": 3}}, "newText": "y"},
            ]},
        ]});
        let files = workspace_edit(&edit).expect("documentChanges");
        assert_eq!((files.len(), files[0].edits.len()), (1, 1));
        assert_eq!(files[0].version, Some(3));
    }

    #[test]
    fn hostile_workspace_edits_are_refused() {
        assert!(
            workspace_edit(&json!({"documentChanges": [{"kind": "delete", "uri": "file:///a"}]}))
                .is_err()
        );
        assert!(workspace_edit(&json!({"changes": {"http://x/a": []}})).is_err());
        assert!(workspace_edit(&json!({"changes": {"file:///a": [{"newText": "x"}]}})).is_err());
        assert!(workspace_edit(&json!({"changes": {"file:///a": 5}})).is_err());
        assert_eq!(workspace_edit(&json!(null)).expect("empty").len(), 0);
    }

    #[test]
    fn offsets_follow_the_edits_around_them() {
        let edits = [(Range::new(2, 4), 5), (Range::new(10, 10), 3)];
        assert_eq!(follow(0, &edits), 0);
        assert_eq!(follow(3, &edits), 7, "inside an edit: its new end");
        assert_eq!(follow(6, &edits), 9);
        assert_eq!(follow(12, &edits), 18);
    }
}
