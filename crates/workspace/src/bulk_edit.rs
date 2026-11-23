//! Applying edits to several documents as a whole, shared by rename (a
//! language server's `WorkspaceEdit`) and replace across files.
//!
//! Studied in VS Code and reimplemented, see `ATTRIBUTION.md`: its bulk
//! edit service applies the edits of every resource as one operation, one
//! undo element per file, and aborts the lot when a file cannot take them
//! (`src/vs/workbench/contrib/bulkEdit/browser/bulkTextEdits.ts:216-218`).
//!
//! Every file's edits are checked before the first file changes: the edits
//! of a file are sorted and must not overlap. A file's edits are one undo
//! step of its document, which leaves the cursor where the text around it
//! moved.

use crate::{DocumentId, Error, Range, Selection, Workspace};
use std::fmt;
use std::path::PathBuf;

/// Why a set of edits was not applied. Nothing was changed, and every
/// variant names the file it is about.
#[derive(Debug)]
pub(crate) enum EditsError {
    /// A file could not be opened to take its edits (the error names it).
    Open(Error),
    /// A document was closed while the edits were planned.
    Closed(Error),
    /// Two edits of the file overlap.
    Overlap(PathBuf),
    /// The file changed since the edits were computed.
    Changed(PathBuf),
    /// The document refused its edits.
    Refused { path: PathBuf, source: Error },
}

impl fmt::Display for EditsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(err) | Self::Closed(err) => err.fmt(f),
            Self::Overlap(path) => write!(
                f,
                "{} has overlapping edits; nothing was changed",
                path.display()
            ),
            Self::Changed(path) => write!(
                f,
                "{} changed since the edit was computed; nothing was changed",
                path.display()
            ),
            Self::Refused { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for EditsError {}

/// What was applied to one document: the char ranges that were replaced
/// (sorted, not overlapping) and how many chars replaced each, in the
/// document's line ending.
#[derive(Debug)]
pub(crate) struct Applied {
    pub document: DocumentId,
    pub edits: Vec<(Range, usize)>,
}

/// Where char `offset` goes when `edits` (sorted, not overlapping, as char
/// ranges and the chars that replace them) are applied: past an edit it
/// moves with the text, inside one it keeps its distance from the start of
/// the edit, within the new text.
pub(crate) fn follow(offset: usize, edits: &[(Range, usize)]) -> usize {
    let (mut added, mut removed) = (0_usize, 0_usize);
    for (range, inserted) in edits {
        if range.end <= offset {
            added = added.saturating_add(*inserted);
            removed = removed.saturating_add(range.len());
        } else if range.start < offset {
            let inside = (offset - range.start).min(*inserted);
            return range
                .start
                .saturating_add(added)
                .saturating_sub(removed)
                .saturating_add(inside);
        } else {
            break;
        }
    }
    offset.saturating_add(added).saturating_sub(removed)
}

impl Workspace {
    /// Applies `planned` (a document and the edits to its text) as a
    /// whole: every file's edits are checked before the first document
    /// changes. Returns what changed, for the documents whose text did.
    pub(crate) fn apply_planned(
        &mut self,
        planned: Vec<(DocumentId, Vec<(Range, String)>)>,
    ) -> Result<Vec<Applied>, EditsError> {
        let mut checked = Vec::with_capacity(planned.len());
        for (id, mut edits) in planned {
            let doc = self
                .document(id)
                .ok_or(EditsError::Closed(Error::UnknownDocument(id)))?;
            edits.sort_by_key(|(range, _)| (range.start, range.end));
            if edits.windows(2).any(|w| w[1].0.start < w[0].0.end) {
                return Err(EditsError::Overlap(doc.path().to_owned()));
            }
            checked.push((id, edits, doc.primary_selection()));
        }
        let mut applied = Vec::new();
        for (id, edits, primary) in checked {
            let path = self
                .document(id)
                .map(|d| d.path().to_owned())
                .unwrap_or_default();
            let refused = |source: Error| EditsError::Refused {
                path: path.clone(),
                source,
            };
            let mut sized = Vec::with_capacity(edits.len());
            let mut eol_edits = Vec::with_capacity(edits.len());
            for (range, text) in edits {
                let text = self.with_document_eol(id, &text).map_err(refused)?;
                sized.push((range, text.chars().count()));
                eol_edits.push((range, text));
            }
            let after =
                Selection::new(follow(primary.anchor, &sized), follow(primary.head, &sized));
            let before = self.document(id).map(crate::Document::version);
            self.edit(id, |editor| editor.replace_ranges(eol_edits, after))
                .map_err(refused)?;
            if self.document(id).map(crate::Document::version) != before {
                applied.push(Applied {
                    document: id,
                    edits: sized,
                });
            }
        }
        Ok(applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_follow_the_edits_around_them() {
        let edits = [(Range::new(2, 4), 5), (Range::new(10, 10), 3)];
        assert_eq!(follow(0, &edits), 0);
        assert_eq!(follow(3, &edits), 3, "inside an edit: same distance in");
        assert_eq!(follow(4, &[(Range::new(2, 6), 1)]), 3, "or its new end");
        assert_eq!(follow(6, &edits), 9);
        assert_eq!(follow(12, &edits), 18);
    }
}
