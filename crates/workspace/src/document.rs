//! One open document: its text, how it is stored on disk, and its syntax
//! tree.
//!
//! A Rust document keeps a Tree-sitter tree in step with every change to
//! its text, whatever made it: typing, deleting, replacing, several
//! cursors, undo and redo, a reload from disk or a line-ending change all
//! go through the editor's byte-edit log. As in VS Code, a file too large
//! for tokenization when opened (over 20 MB or 300,000 lines) is never
//! highlighted, even if it shrinks later
//! (`src/vs/editor/common/model/textModel.ts:189-190`, `:334-343`; studied
//! and reimplemented, see `ATTRIBUTION.md`). VS Code counts UTF-16 code
//! units; rustode counts chars.

use crate::language::{self, Language};
use rustode_core::{EditError, Editor, Position, Range, Selection};
use rustode_fs::{EncodeError, Encoding, Eol};
use rustode_syntax::{Syntax, TokenKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Length and 64-bit hash of a file's bytes. Two different contents share a
/// stamp with negligible probability (about 2^-64 per comparison).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiskStamp {
    len: usize,
    hash: u64,
}

impl DiskStamp {
    pub(crate) fn of(bytes: &[u8]) -> Self {
        use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
        let hash = BuildHasherDefault::<DefaultHasher>::default().hash_one(bytes);
        Self {
            len: bytes.len(),
            hash,
        }
    }
}

/// Identifies an open document within a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentId(pub(crate) usize);

/// An open document.
///
/// Line endings are kept exactly as read, so saving an unchanged file
/// reproduces its bytes even when it mixes LF and CRLF.
#[derive(Debug)]
pub struct Document {
    path: PathBuf,
    editor: Editor,
    encoding: Encoding,
    bom: bool,
    saved_version: u64,
    /// A fingerprint of the bytes last read from or written to the file:
    /// what "unchanged on disk" is measured against. A fingerprint rather
    /// than the bytes, so a large file is not held twice.
    pub(crate) disk: DiskStamp,
    /// Whether the file is gone from disk (deleted or renamed away).
    pub(crate) orphaned: bool,
    /// The file's bytes when it changed under unsaved edits, until resolved.
    pub(crate) conflict: Option<Vec<u8>>,
    /// The line ending new line breaks use, and whether the text mixes both
    /// kinds; kept rather than rescanning the text on every call.
    eol: (Eol, bool),
    /// The line-ending state at versions created by an EOL change or a
    /// reload, so undo and redo restore it (VS Code's `pushEOL` is undoable).
    eol_at: HashMap<u64, (Eol, bool)>,
    language: &'static Language,
    /// The syntax tree, for languages with a grammar and files not too
    /// large for it.
    syntax: Option<Syntax>,
}

/// A run of highlighted text: char offsets and what the text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    /// The chars of the run.
    pub range: Range,
    /// What the text is.
    pub kind: TokenKind,
}

/// More chars than this and the document is never highlighted (VS Code's
/// `LARGE_FILE_SIZE_THRESHOLD`, `textModel.ts:189`).
const TOKENIZATION_CHAR_LIMIT: usize = 20 * 1024 * 1024;
/// More lines than this and the document is never highlighted
/// (`LARGE_FILE_LINE_COUNT_THRESHOLD`, `textModel.ts:190`).
const TOKENIZATION_LINE_LIMIT: usize = 300 * 1000;

impl Document {
    /// Decodes a file's bytes, without copying valid UTF-8 (large files).
    pub(crate) fn from_owned_bytes(path: PathBuf, bytes: Vec<u8>) -> Self {
        let disk = DiskStamp::of(&bytes);
        Self::from_decoded(path, rustode_fs::decode_owned(bytes), disk)
    }

    fn from_decoded(path: PathBuf, decoded: rustode_fs::Decoded, disk: DiskStamp) -> Self {
        let rustode_fs::Decoded {
            text,
            encoding,
            bom,
        } = decoded;
        let info = rustode_fs::detect_eol(&text);
        let name = path
            .file_name()
            .map_or_else(Default::default, |n| n.to_string_lossy().into_owned());
        let language = language::detect(&name);
        let mut editor = Editor::new(&text);
        let syntax = syntax_for(language.id, &mut editor);
        Self {
            language,
            syntax,
            eol: (info.dominant, info.mixed),
            eol_at: HashMap::new(),
            path,
            editor,
            encoding,
            bom,
            saved_version: 0,
            disk,
            orphaned: false,
            conflict: None,
        }
    }

    /// The file this document belongs to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The whole text.
    #[must_use]
    pub fn text(&self) -> String {
        self.editor.buffer().text()
    }

    /// Length of the text in chars.
    #[must_use]
    pub fn len_chars(&self) -> usize {
        self.editor.buffer().len_chars()
    }

    /// The cursor, as a char offset.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.editor.cursor()
    }

    /// All selections, sorted and never overlapping.
    #[must_use]
    pub fn selections(&self) -> &[Selection] {
        self.editor.selections()
    }

    /// The primary selection, the one a frontend reveals.
    #[must_use]
    pub fn primary_selection(&self) -> Selection {
        self.editor.primary()
    }

    /// Whether the text differs from the last saved (or opened) state.
    /// Undoing back to that state makes the document clean again.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.editor.version() != self.saved_version
    }

    pub(crate) fn editor(&self) -> &Editor {
        &self.editor
    }

    pub(crate) fn editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }

    /// Records that the text is now what `bytes`, the file's content, holds.
    pub(crate) fn mark_saved(&mut self, bytes: &[u8]) {
        self.saved_version = self.editor.version();
        self.disk = DiskStamp::of(bytes);
        self.orphaned = false;
        self.conflict = None;
    }

    /// Makes no version of the text count as saved, so the document stays
    /// dirty until it is saved again, even if undone to its old state.
    pub(crate) fn forget_saved_version(&mut self) {
        self.saved_version = u64::MAX;
    }

    pub(crate) fn set_stored_encoding(&mut self, encoding: Encoding, bom: bool) {
        self.encoding = encoding;
        self.bom = bom;
    }

    /// Whether the file was deleted or renamed away since it was opened or
    /// saved. The text is kept; saving writes the file again.
    #[must_use]
    pub fn is_orphaned(&self) -> bool {
        self.orphaned
    }

    /// Whether the file changed on disk under unsaved edits and the user has
    /// not chosen yet which version to keep.
    #[must_use]
    pub fn has_conflict(&self) -> bool {
        self.conflict.is_some()
    }

    /// The encoding used on save.
    #[must_use]
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Whether a byte order mark is written on save.
    #[must_use]
    pub fn has_bom(&self) -> bool {
        self.bom
    }

    /// The dominant line ending.
    #[must_use]
    pub fn eol(&self) -> Eol {
        self.eol.0
    }

    /// Whether the text mixes LF and CRLF.
    #[must_use]
    pub fn has_mixed_eol(&self) -> bool {
        self.eol.1
    }

    pub(crate) fn encoded(&self) -> Result<Vec<u8>, EncodeError> {
        rustode_fs::encode(&self.text(), self.encoding, self.bom)
    }

    pub(crate) fn set_encoding(
        &mut self,
        encoding: Encoding,
        bom: bool,
    ) -> Result<(), EncodeError> {
        rustode_fs::encode(&self.text(), encoding, bom)?;
        self.encoding = encoding;
        self.bom = bom && encoding != Encoding::Windows1252;
        Ok(())
    }

    /// Rewrites every line ending as `eol`, as one undoable edit. Returns
    /// whether the text changed.
    pub(crate) fn set_eol(&mut self, eol: Eol) -> Result<bool, EditError> {
        let text = self.text();
        let normalized = rustode_fs::normalize_eol(&text, eol);
        if normalized == text {
            self.eol = (eol, false);
            return Ok(false);
        }
        let before = self.editor.version();
        let changed = self
            .editor
            .replace(Range::new(0, self.len_chars()), &normalized)?;
        self.record_eol(before, (eol, false));
        Ok(changed)
    }

    /// Rescans the text for its line endings after it was replaced from disk.
    pub(crate) fn rescan_eol(&mut self, before_version: u64) {
        let info = rustode_fs::detect_eol(&self.text());
        self.record_eol(before_version, (info.dominant, info.mixed));
    }

    fn record_eol(&mut self, before_version: u64, after: (Eol, bool)) {
        self.eol_at.insert(before_version, self.eol);
        self.eol_at.insert(self.editor.version(), after);
        self.eol = after;
    }

    /// After undo or redo: the line-ending state of the version reached, if
    /// an EOL change or a reload created it.
    pub(crate) fn sync_eol(&mut self) {
        if let Some(state) = self.eol_at.get(&self.editor.version()) {
            self.eol = *state;
        }
    }

    /// The version of the text: every edit gives a new one, and undo and
    /// redo return to earlier ones. A frontend can keep what it derived from
    /// the text (find matches, for example) until the version changes.
    #[must_use]
    pub fn version(&self) -> u64 {
        self.editor.version()
    }

    /// The number of lines; an empty document has one.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.editor.buffer().len_lines()
    }

    /// The text of `line` without its line break, or an empty string past the
    /// last line. Copies only that line, so frontends can draw what is visible.
    #[must_use]
    pub fn line(&self, line: usize) -> String {
        if line < self.line_count() {
            self.editor.buffer().line_text(line)
        } else {
            String::new()
        }
    }

    /// The line and column (both 0-based, columns in chars) of the primary cursor.
    #[must_use]
    pub fn cursor_position(&self) -> Position {
        self.editor.position_of(self.cursor())
    }

    /// The line and column of char `offset`, clamped to the end of the text.
    #[must_use]
    pub fn position_of(&self, offset: usize) -> Position {
        self.editor.position_of(offset.min(self.len_chars()))
    }

    /// The char offset of `position`, clamping the line and the column.
    #[must_use]
    pub fn offset_of(&self, position: Position) -> usize {
        if position.line >= self.line_count() {
            return self.len_chars();
        }
        self.editor
            .buffer()
            .offset_of(position.line, position.column)
    }

    /// The encoding as VS Code's status bar shows it (`labelShort` in
    /// `src/vs/workbench/services/textfile/common/encoding.ts:547-578`, shown
    /// by `src/vs/workbench/browser/parts/editor/editorStatus.ts:903`).
    #[must_use]
    pub fn encoding_name(&self) -> &'static str {
        match (self.encoding, self.bom) {
            (Encoding::Utf8, false) => "UTF-8",
            (Encoding::Utf8, true) => "UTF-8 with BOM",
            (Encoding::Utf16Le, _) => "UTF-16 LE",
            (Encoding::Utf16Be, _) => "UTF-16 BE",
            (Encoding::Windows1252, _) => "Windows 1252",
        }
    }

    /// The language id used by settings overrides, e.g. `rust`.
    #[must_use]
    pub fn language_id(&self) -> &'static str {
        self.language.id
    }

    /// The language name shown in the status bar, e.g. `Rust`.
    #[must_use]
    pub fn language_name(&self) -> &'static str {
        self.language.name
    }

    /// The highlighted runs of `lines` (zero-based, end excluded), sorted
    /// and never overlapping; text in no run, and every document without a
    /// syntax tree, is plain. Only those lines are queried, so a frontend
    /// asks for the ones it shows.
    #[must_use]
    pub fn highlights(&self, lines: std::ops::Range<usize>) -> Vec<Highlight> {
        let Some(syntax) = &self.syntax else {
            return Vec::new();
        };
        let buffer = self.editor.buffer();
        let byte = |line| buffer.char_to_byte(self.offset_of(Position::new(line, 0)));
        syntax
            .highlights(buffer, byte(lines.start)..byte(lines.end))
            .into_iter()
            .map(|span| Highlight {
                range: Range::new(
                    buffer.byte_to_char(span.start),
                    buffer.byte_to_char(span.end),
                ),
                kind: span.kind,
            })
            .collect()
    }

    /// The syntax tree as an S-expression, if the document has one.
    #[must_use]
    pub fn syntax_tree(&self) -> Option<String> {
        self.syntax.as_ref().map(Syntax::sexp)
    }

    /// Replays the text's changes since the last call on the syntax tree.
    pub(crate) fn sync_syntax(&mut self) {
        if let Some(syntax) = self.syntax.as_mut() {
            let edits = self.editor.take_byte_edits();
            syntax.edit(&edits, self.editor.buffer());
        }
    }
}

/// A syntax tree for a new document's text, logging the editor's changes
/// so the tree can follow them; `None` when the language has no grammar or
/// the text is too large.
fn syntax_for(language_id: &str, editor: &mut Editor) -> Option<Syntax> {
    let buffer = editor.buffer();
    if buffer.len_chars() > TOKENIZATION_CHAR_LIMIT || buffer.len_lines() > TOKENIZATION_LINE_LIMIT
    {
        return None;
    }
    let syntax = Syntax::new(language_id, buffer)?;
    editor.record_byte_edits();
    Some(syntax)
}
