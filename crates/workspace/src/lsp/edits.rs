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
use crate::bulk_edit::EditsError;
use crate::{DocumentId, Error, Range, Workspace};
use serde_json::{Value, json};
use std::fmt;
use std::path::PathBuf;

/// What is unusable in the edits a language server answered with.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AnswerProblem {
    NotAList,
    TooManyEdits,
    BadEdit,
    NotAFile(String),
    FileOperation(String),
    NoFile,
    TooManyFiles,
}

impl fmt::Display for AnswerProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAList => f.write_str("the server's edits are not a list"),
            Self::TooManyEdits => {
                write!(f, "the server sent more than {MAX_EDITS} edits in one file")
            }
            Self::BadEdit => f.write_str("an edit of the server has no range or no new text"),
            Self::NotAFile(uri) => write!(f, "the server's edit names {uri}, which is not a file"),
            Self::FileOperation(kind) => write!(
                f,
                "the server asked to {kind} a file; creating, renaming and deleting files \
                 are not supported yet"
            ),
            Self::NoFile => f.write_str("an edit of the server names no file"),
            Self::TooManyFiles => {
                write!(f, "the server's edit changes more than {MAX_FILES} files")
            }
        }
    }
}

/// Why the edits of a language server were not applied.
#[derive(Debug)]
pub(crate) enum EditFailure {
    /// The server's answer is unusable.
    Answer(AnswerProblem),
    /// The workspace could not apply the edits.
    Apply(EditsError),
}

impl fmt::Display for EditFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Answer(problem) => problem.fmt(f),
            Self::Apply(err) => err.fmt(f),
        }
    }
}

impl From<AnswerProblem> for EditFailure {
    fn from(problem: AnswerProblem) -> Self {
        Self::Answer(problem)
    }
}

impl From<EditsError> for EditFailure {
    fn from(err: EditsError) -> Self {
        Self::Apply(err)
    }
}

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
fn text_edits(list: &Value) -> Result<Vec<TextEdit>, AnswerProblem> {
    let list = list.as_array().ok_or(AnswerProblem::NotAList)?;
    if list.len() > MAX_EDITS {
        return Err(AnswerProblem::TooManyEdits);
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
                _ => Err(AnswerProblem::BadEdit),
            }
        })
        .collect()
}

/// The files a `WorkspaceEdit` changes, one entry per file.
fn workspace_edit(edit: &Value) -> Result<Vec<FileEdits>, AnswerProblem> {
    let mut files: Vec<FileEdits> = Vec::new();
    let mut add = |uri_text: &str, version: Option<i64>, edits: Vec<TextEdit>| {
        let path =
            uri::to_path(uri_text).ok_or_else(|| AnswerProblem::NotAFile(uri_text.to_owned()))?;
        if let Some(file) = files.iter_mut().find(|f| f.path == path) {
            file.edits.extend(edits);
        } else {
            files.push(FileEdits {
                path,
                version,
                edits,
            });
        }
        Ok::<(), AnswerProblem>(())
    };
    if let Some(changes) = edit["documentChanges"].as_array() {
        for change in changes {
            if let Some(kind) = change.get("kind") {
                return Err(AnswerProblem::FileOperation(
                    kind.as_str().unwrap_or("change").to_owned(),
                ));
            }
            let document = &change["textDocument"];
            let uri_text = document["uri"].as_str().ok_or(AnswerProblem::NoFile)?;
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
        return Err(AnswerProblem::TooManyFiles);
    }
    Ok(files)
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
        let applied = files
            .map_err(EditFailure::from)
            .and_then(|files| Ok(self.apply_edits(&files, encoding)?));
        match (applied, kind) {
            (Err(why), _) => self.notice(format!("The {what} was not applied: {why}")),
            (Ok((_, 0)), Kind::Format) => self.notice("Nothing to format".to_owned()),
            (Ok(_), Kind::Format) => self.notice("Formatted the document".to_owned()),
            (Ok((files, edits)), _) => self.notice(format!(
                "Renamed to '{}': {edits} edit(s) in {files} file(s)",
                op.name
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
    ) -> Result<(usize, usize), EditsError> {
        for file in files {
            let expected = file.version;
            let open = self.find_open(&file.path);
            if let (Some(expected), Some(id)) = (expected, open)
                && let Some(tracked) = self.lsp.tracked.get(&id)
                && tracked.version != expected
            {
                return Err(EditsError::Changed(file.path.clone()));
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
    ) -> Result<(usize, usize), EditsError> {
        let mut planned = Vec::with_capacity(files.len());
        for file in files {
            let id = self.open(&file.path).map_err(EditsError::Open)?;
            let doc = self
                .document(id)
                .ok_or(EditsError::Closed(Error::UnknownDocument(id)))?;
            let edits: Vec<(Range, String)> = file
                .edits
                .iter()
                .map(|edit| {
                    let start = super::offset_at(doc, edit.start.0, edit.start.1, encoding);
                    let end = super::offset_at(doc, edit.end.0, edit.end.1, encoding).max(start);
                    (Range::new(start, end), edit.text.clone())
                })
                .collect();
            planned.push((id, edits));
        }
        let applied = self.apply_planned(planned)?;
        Ok((applied.len(), applied.iter().map(|a| a.edits.len()).sum()))
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
}
