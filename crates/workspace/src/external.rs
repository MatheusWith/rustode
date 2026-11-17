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
//! then accepts or reverts the local changes (`…:418-442`). rustode raises
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

impl Workspace {
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
            return Err(Error::NoConflict(id));
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
        if self.watcher.is_none() {
            match rustode_fs::Watcher::new() {
                Ok(watcher) => self.watcher = Some(watcher),
                Err(err) => {
                    tracing::warn!(%err, "no file watcher; external changes go unnoticed");
                    return;
                }
            }
        }
        if let Some(watcher) = self.watcher.as_mut()
            && let Err(err) = watcher.watch_file(path)
        {
            tracing::warn!(path = %path.display(), %err, "cannot watch");
        }
    }

    fn check(&mut self, id: DocumentId, renamed_to: Option<PathBuf>) -> Result<(), Error> {
        let document = self.document_mut(id)?;
        let path = document.path().to_owned();
        let bytes = match rustode_fs::read_file(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if !document.orphaned {
                    document.orphaned = true;
                    tracing::debug!(path = %path.display(), "file gone");
                    self.events.push(match renamed_to {
                        Some(to) => Event::Renamed { id, to },
                        None => Event::Deleted { id },
                    });
                }
                return Ok(());
            }
            Err(source) => return Err(Error::Io { path, source }),
        };
        if document.orphaned {
            document.orphaned = false;
            self.events.push(Event::Restored { id });
        }
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
            tracing::debug!(path = %path.display(), "conflict");
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
    let changed = document
        .editor_mut()
        .replace(range, &inserted)
        .map_err(Error::Edit)?;
    document.set_stored_encoding(decoded.encoding, decoded.bom);
    document.mark_saved(bytes);
    Ok(changed)
}
