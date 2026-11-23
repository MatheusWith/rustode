//! Files changed, deleted or renamed by other programs.
//!
//! The behavior follows VS Code (Code - OSS, studied in the source tree and
//! reimplemented; see `ATTRIBUTION.md`), with paths under
//! `src/vs/workbench/services/textfile/common/`:
//!
//! - a clean document reloads when its file changes, and a dirty one never
//!   does (`textFileEditorModelManager.ts:134-147`);
//! - the reload is pushed as an undoable edit, and nothing happens when the
//!   text is already equal (`src/vs/editor/common/services/modelService.ts:365-387`);
//!   after it the document is clean (`textFileEditorModel.ts:650`);
//! - a deleted file leaves the document open and "orphaned", and a file that
//!   comes back clears that (`textFileEditorModel.ts:158-201`); saving clears
//!   it too (`textFileEditorModel.ts:1063-1064`);
//! - once the disk moved on, the old saved text no longer counts as saved,
//!   even when undone back to (`textFileEditorModel.ts:404-406`).
//!
//! VS Code finds the conflict only when a save fails because the file is
//! newer, and offers "Compare" and "Overwrite"
//! (`src/vs/workbench/contrib/files/browser/editors/textFileSaveErrorHandler.ts:113-135`),
//! then accepts or reverts the local changes (`…:410-442`; ids
//! `workbench.files.action.saveIgnoreModifiedSince` at `:387`,
//! `…acceptLocalChanges` and `…revertLocalChanges` in `fileActions.contribution.ts:199-200`). rustode raises
//! the conflict as soon as the watcher reports the change, and resolves it
//! with the same two choices.
//!
//! "Changed" always means the bytes differ from the ones last read or
//! written, so a touch, or rustode's own save, is never an external change.

use crate::document::DiskStamp;
use crate::{Document, DocumentId, Error, Event, Range, Workspace};
use rustode_fs::FsChange;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// How to settle a conflict between unsaved edits and a changed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Replace the text with the file's, as one undo step, so the edits can
    /// still be recovered with undo. VS Code's "Revert".
    ReloadTheirs,
    /// Keep the edits; the next save overwrites the file. VS Code's
    /// "Overwrite".
    KeepMine,
}

/// One way out of a conflict, as the prompt offers it: a registered command
/// ([`COMMANDS`](crate::COMMANDS)) and the short word on its button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConflictChoice {
    /// The command's VS Code id; [`Workspace::execute`] runs it.
    pub command: &'static str,
    /// The button or key hint: VS Code's own word for the action.
    pub label: &'static str,
}

/// What a frontend shows when a save hit a conflict: the sentence and the
/// choices. The workspace decides both; the frontends only draw them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictPrompt {
    /// The document in conflict.
    pub document: DocumentId,
    /// VS Code's "Failed to save" sentence (`staleSaveError`,
    /// `textFileSaveErrorHandler.ts:129`), without the compare choice.
    pub message: String,
    /// The choices, in VS Code's order.
    pub choices: Vec<ConflictChoice>,
}

/// VS Code's "Overwrite" (`SaveModelIgnoreModifiedSinceAction`,
/// `textFileSaveErrorHandler.ts:387`).
pub(crate) const OVERWRITE: &str = "workbench.files.action.saveIgnoreModifiedSince";
/// VS Code's "Discard your changes and revert to file contents"
/// (`fileActions.contribution.ts:200`).
pub(crate) const REVERT: &str = "workbench.files.action.revertLocalChanges";

impl Workspace {
    /// The prompt for a document whose file changed under unsaved edits, or
    /// `None` when it has no conflict. Compare (VS Code's
    /// `workbench.files.action.resolveConflict`) needs a diff editor, which
    /// rustode does not have, so it is not offered.
    #[must_use]
    pub fn conflict_prompt(&self, id: DocumentId) -> Option<ConflictPrompt> {
        let document = self.document(id).filter(|d| d.has_conflict())?;
        Some(ConflictPrompt {
            document: id,
            message: format!(
                "Failed to save '{}': The content of the file is newer. Please overwrite the \
                 content of the file with your changes or revert to the file contents.",
                document.name()
            ),
            choices: vec![
                ConflictChoice {
                    command: OVERWRITE,
                    label: "Overwrite",
                },
                ConflictChoice {
                    command: REVERT,
                    label: "Revert",
                },
            ],
        })
    }

    /// Keeps the document's text and writes it over the file.
    pub(crate) fn overwrite_conflict(&mut self, id: DocumentId) -> Result<(), Error> {
        self.resolve_conflict(id, Resolution::KeepMine)?;
        self.save(id)
    }

    /// Processes the file changes the watcher saw since the last call,
    /// reloading clean documents and raising conflicts for dirty ones.
    /// Frontends call it from their event loop; the outcome is reported as
    /// [`Event`]s.
    pub fn poll_external_changes(&mut self) {
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        let changes = watcher.poll();
        if changes.is_empty() {
            return;
        }
        let mut touched = HashSet::new();
        let mut renamed = HashMap::new();
        let mut rescan = false;
        for change in changes {
            match change {
                FsChange::Changed(path) => {
                    touched.insert(path);
                }
                FsChange::Renamed { from, to } => {
                    touched.insert(to.clone());
                    touched.insert(from.clone());
                    renamed.insert(from, to);
                }
                FsChange::Rescan => rescan = true,
            }
        }
        for index in 0..self.documents.len() {
            let id = DocumentId(index);
            let Some(path) = self
                .document(id)
                .and_then(|d| std::path::absolute(d.path()).ok())
            else {
                continue;
            };
            if rescan || touched.contains(&path) {
                let to = renamed.get(&path).cloned();
                if let Err(err) = self.check(id, to) {
                    tracing::warn!(%err, "external change check failed");
                }
            }
        }
    }

    /// Compares one document with its file right now, without waiting for
    /// the watcher, and reacts as [`poll_external_changes`](Self::poll_external_changes)
    /// would.
    ///
    /// # Errors
    /// When there is no such document, or the file exists but cannot be read.
    pub fn check_disk(&mut self, id: DocumentId) -> Result<(), Error> {
        self.check(id, None)
    }

    /// Settles a conflict raised by an external change.
    ///
    /// # Errors
    /// When there is no such document, it has no conflict, or (for
    /// [`Resolution::ReloadTheirs`]) the file cannot be read.
    pub fn resolve_conflict(
        &mut self,
        id: DocumentId,
        resolution: Resolution,
    ) -> Result<(), Error> {
        let document = self.document_mut(id)?;
        let Some(theirs) = document.conflict.take() else {
            return Err(Error::NoConflict {
                id,
                name: document.name(),
            });
        };
        match resolution {
            Resolution::KeepMine => {
                document.disk = DiskStamp::of(&theirs);
                document.forget_saved_version();
                Ok(())
            }
            Resolution::ReloadTheirs => {
                let path = document.path().to_owned();
                let bytes = rustode_fs::read_file(&path).map_err(|source| {
                    document.conflict = Some(theirs);
                    Error::Io {
                        path: path.clone(),
                        source,
                    }
                })?;
                self.reload(id, &bytes)
            }
        }
    }

    pub(crate) fn watch(&mut self, path: &Path) {
        if let Some(watcher) = self.ensure_watcher()
            && let Err(err) = watcher.watch_file(path)
        {
            tracing::warn!(path = %path.display(), %err, "cannot watch");
        }
    }

    /// The file watcher, created on first use; `None` if the platform has none.
    fn ensure_watcher(&mut self) -> Option<&mut rustode_fs::Watcher> {
        if self.watcher.is_none() {
            match rustode_fs::Watcher::new() {
                Ok(watcher) => self.watcher = Some(watcher),
                Err(err) => {
                    tracing::warn!(%err, "no file watcher; external changes go unnoticed");
                }
            }
        }
        self.watcher.as_mut()
    }

    fn check(&mut self, id: DocumentId, renamed_to: Option<PathBuf>) -> Result<(), Error> {
        match self.read_or_orphan(id, renamed_to)? {
            Some(bytes) => self.compare_with_disk(id, bytes),
            None => Ok(()),
        }
    }

    /// The file's bytes; `None` when it is gone, after marking the document
    /// orphaned (once). A file that comes back un-orphans it.
    fn read_or_orphan(
        &mut self,
        id: DocumentId,
        renamed_to: Option<PathBuf>,
    ) -> Result<Option<Vec<u8>>, Error> {
        let document = self.document_mut(id)?;
        let path = document.path().to_owned();
        match rustode_fs::read_file(&path) {
            Ok(bytes) => {
                if document.orphaned {
                    document.orphaned = false;
                    self.events.push(Event::Restored { id });
                }
                Ok(Some(bytes))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if !document.orphaned {
                    document.orphaned = true;
                    tracing::debug!(path = %path.display(), "file gone");
                    self.events.push(match renamed_to {
                        Some(to) => Event::Renamed { id, to },
                        None => Event::Deleted { id },
                    });
                }
                Ok(None)
            }
            Err(source) => Err(Error::Io { path, source }),
        }
    }

    /// Reloads a clean document, or raises a conflict for a dirty one, when
    /// the bytes on disk differ from those last read or written.
    fn compare_with_disk(&mut self, id: DocumentId, bytes: Vec<u8>) -> Result<(), Error> {
        let document = self.document_mut(id)?;
        if DiskStamp::of(&bytes) == document.disk {
            // A touch, our own save, or the file went back to what we know.
            document.conflict = None;
            return Ok(());
        }
        if !document.is_dirty() || rustode_fs::decode(&bytes).text == document.text() {
            return self.reload(id, &bytes);
        }
        if document.conflict.as_ref() != Some(&bytes) {
            tracing::debug!(path = %document.path().display(), "conflict");
            document.conflict = Some(bytes);
            document.forget_saved_version();
            self.events.push(Event::Conflict { id });
        }
        Ok(())
    }

    /// Makes the document hold `bytes`, the file's content, as one undo step.
    fn reload(&mut self, id: DocumentId, bytes: &[u8]) -> Result<(), Error> {
        let document = self.document_mut(id)?;
        let changed = replace_changed_part(document, bytes)?;
        self.changed(id, changed);
        self.events.push(Event::Reloaded { id });
        Ok(())
    }
}

/// Replaces only the part of the text that differs from `bytes`, so the
/// cursor keeps its place when it is outside that part. Returns whether the
/// text changed.
fn replace_changed_part(document: &mut Document, bytes: &[u8]) -> Result<bool, Error> {
    let decoded = rustode_fs::decode(bytes);
    let old: Vec<char> = document.text().chars().collect();
    let new: Vec<char> = decoded.text.chars().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let inserted: String = new[prefix..new.len() - suffix].iter().collect();
    let range = Range::new(prefix, old.len() - suffix);
    let before = document.version();
    let changed = document
        .editor_mut()
        .replace(range, &inserted)
        .map_err(Error::Edit)?;
    document.rescan_eol(before);
    document.set_stored_encoding(decoded.encoding, decoded.bom);
    document.mark_saved(bytes);
    Ok(changed)
}
