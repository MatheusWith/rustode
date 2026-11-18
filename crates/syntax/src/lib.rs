//! Tree-sitter parsing, incremental reparse and highlighting.
//!
//! A [`Syntax`] holds one document's tree. Each change to the text is
//! replayed on the tree ([`Syntax::edit`]) and the text parsed again with
//! the old tree, so only the changed part is parsed anew. Highlights are
//! queried for a byte range, the visible lines, never the whole text.
//!
//! When several captures overlap, the innermost node wins, and of the
//! captures of one span the first pattern of the query wins, as in
//! Tree-sitter's own highlighter, the convention `tree-sitter-rust` writes
//! its query for. (VS Code's tree-sitter queries are written the other way
//! round, with the last pattern winning:
//! `src/vs/editor/common/languages/highlights/typescript.scm:1`.)

mod grammar;
mod token;

pub use token::{DEFAULT_FOREGROUND, TokenKind};

use grammar::Grammar;
use rustode_core::{Buffer, ByteEdit, BytePosition};
use std::fmt;
use std::ops::Range;
use tree_sitter::{InputEdit, Node, Parser, Point, QueryCursor, StreamingIterator, Tree};

/// Text a tree is parsed from, read in chunks so it is never copied whole.
pub trait Text {
    /// Length in bytes.
    fn len_bytes(&self) -> usize;
    /// The text from byte `byte` on, up to wherever it is stored contiguously;
    /// empty at the end.
    fn chunk_at(&self, byte: usize) -> &[u8];
    /// The text between two bytes, in chunks.
    fn chunks(&self, range: Range<usize>) -> impl Iterator<Item = &[u8]>;
}

impl Text for Buffer {
    fn len_bytes(&self) -> usize {
        Buffer::len_bytes(self)
    }

    fn chunk_at(&self, byte: usize) -> &[u8] {
        self.chunk_at_byte(byte).as_bytes()
    }

    fn chunks(&self, range: Range<usize>) -> impl Iterator<Item = &[u8]> {
        self.byte_chunks(range.start, range.end).map(str::as_bytes)
    }
}

impl Text for str {
    fn len_bytes(&self) -> usize {
        self.len()
    }

    fn chunk_at(&self, byte: usize) -> &[u8] {
        self.as_bytes().get(byte..).unwrap_or_default()
    }

    fn chunks(&self, range: Range<usize>) -> impl Iterator<Item = &[u8]> {
        std::iter::once(self.as_bytes().get(range).unwrap_or_default())
    }
}

/// A run of highlighted text, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// The first byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
    /// What the text is.
    pub kind: TokenKind,
}

/// One document's syntax tree, kept in step with its text.
pub struct Syntax {
    grammar: &'static Grammar,
    parser: Parser,
    tree: Tree,
}

impl fmt::Debug for Syntax {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Syntax")
            .field("root", &self.tree.root_node().kind())
            .finish_non_exhaustive()
    }
}

impl Syntax {
    /// Parses `text` as language `language_id` (`rust`); `None` when rustode
    /// has no grammar for it.
    #[must_use]
    pub fn new(language_id: &str, text: &(impl Text + ?Sized)) -> Option<Self> {
        let grammar = Grammar::for_language(language_id)?;
        let mut parser = Parser::new();
        parser.set_language(&grammar.language).ok()?;
        let tree = parse(&mut parser, text, None)?;
        Some(Self {
            grammar,
            parser,
            tree,
        })
    }

    /// Replays changes made to the text on the tree, oldest first, then
    /// parses `text`, the text after them, reusing what did not change.
    pub fn edit(&mut self, edits: &[ByteEdit], text: &(impl Text + ?Sized)) {
        if edits.is_empty() {
            return;
        }
        for edit in edits {
            self.tree.edit(&InputEdit {
                start_byte: edit.start.offset,
                old_end_byte: edit.old_end.offset,
                new_end_byte: edit.new_end.offset,
                start_position: point(edit.start),
                old_end_position: point(edit.old_end),
                new_end_position: point(edit.new_end),
            });
        }
        if let Some(tree) = parse(&mut self.parser, text, Some(&self.tree)) {
            self.tree = tree;
        }
    }

    /// The tree as an S-expression of its named nodes.
    #[must_use]
    pub fn sexp(&self) -> String {
        self.tree.root_node().to_sexp()
    }

    /// The highlighted runs between bytes `range.start` and `range.end` of
    /// `text`, sorted and never overlapping; text in no run is plain. Nodes
    /// that start before the range or end after it are cut to it.
    #[must_use]
    pub fn highlights(&self, text: &(impl Text + ?Sized), range: Range<usize>) -> Vec<Span> {
        let end = range.end.min(text.len_bytes());
        let start = range.start.min(end);
        if start == end {
            return Vec::new();
        }
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(start..end);
        let node_text = |node: Node<'_>| text.chunks(node.byte_range());
        let mut captures = cursor.captures(&self.grammar.query, self.tree.root_node(), node_text);
        let mut found = Vec::new();
        while let Some((found_match, index)) = captures.next() {
            let capture = found_match.captures()[*index];
            let node = capture.node;
            if let Some(kind) = self.grammar.kind(capture.index, node.kind()) {
                found.push((node.start_byte(), node.end_byte(), kind));
            }
        }
        flatten(found, start, end)
    }
}

/// Non-overlapping runs from captures given in query order: of captures
/// of one span the first wins, and inner spans win over outer ones.
fn flatten(mut found: Vec<(usize, usize, TokenKind)>, start: usize, end: usize) -> Vec<Span> {
    // A stable sort keeps the query order among captures of one span.
    found.sort_by_key(|&(from, to, _)| (from, std::cmp::Reverse(to)));
    found.dedup_by_key(|&mut (from, to, _)| (from, to));
    let mut paint: Vec<Option<TokenKind>> = vec![None; end - start];
    for (from, to, kind) in found {
        let (from, to) = (from.clamp(start, end), to.clamp(start, end));
        paint[from - start..to - start].fill(Some(kind));
    }
    let mut spans: Vec<Span> = Vec::new();
    for (offset, kind) in paint.into_iter().enumerate() {
        let Some(kind) = kind else { continue };
        let at = start + offset;
        match spans.last_mut() {
            Some(last) if last.end == at && last.kind == kind => last.end = at + 1,
            _ => spans.push(Span {
                start: at,
                end: at + 1,
                kind,
            }),
        }
    }
    spans
}

fn point(position: BytePosition) -> Point {
    Point::new(position.line, position.column)
}

fn parse(parser: &mut Parser, text: &(impl Text + ?Sized), old: Option<&Tree>) -> Option<Tree> {
    parser.parse_with_options(&mut |byte, _| text.chunk_at(byte), old, None)
}

/// The S-expression of a fresh parse of `text` as `language_id`, with no
/// earlier tree to reuse; `None` when rustode has no grammar for it.
#[must_use]
pub fn sexp_of(language_id: &str, text: &str) -> Option<String> {
    Syntax::new(language_id, text).map(|syntax| syntax.sexp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<(&str, TokenKind)> {
        let syntax = Syntax::new("rust", text).expect("rust");
        syntax
            .highlights(text, 0..text.len())
            .into_iter()
            .map(|span| (&text[span.start..span.end], span.kind))
            .collect()
    }

    #[test]
    fn inner_captures_win_and_the_first_pattern_wins_a_span() {
        use TokenKind as K;
        assert_eq!(
            spans("#[x] const MAX: u8 = 1;"),
            [
                ("#", K::Attribute),
                ("[", K::Punctuation),
                ("x", K::Variable),
                ("]", K::Punctuation),
                ("const", K::Keyword),
                ("MAX", K::Constant),
                (":", K::Operator),
                ("u8", K::Type),
                ("1", K::Number),
                (";", K::Punctuation),
            ]
        );
    }

    #[test]
    fn a_range_cuts_the_nodes_it_crosses() {
        let text = "/* one\ntwo */ x";
        let syntax = Syntax::new("rust", text).expect("rust");
        assert_eq!(
            syntax.highlights(text, 7..11),
            [Span {
                start: 7,
                end: 11,
                kind: TokenKind::Comment
            }]
        );
        assert!(syntax.highlights(text, 9..9).is_empty(), "empty range");
        assert!(syntax.highlights(text, 99..120).is_empty(), "past the end");
    }

    #[test]
    fn edits_reparse_to_the_tree_of_a_fresh_parse() {
        let mut editor = rustode_core::Editor::new("fn a() {}\n");
        editor.record_byte_edits();
        let mut syntax = Syntax::new("rust", editor.buffer()).expect("rust");
        editor.set_cursor(8).expect("cursor");
        editor.type_text("1");
        syntax.edit(&editor.take_byte_edits(), editor.buffer());
        let text = editor.buffer().text();
        assert_eq!(Some(syntax.sexp()), sexp_of("rust", &text));
        assert!(
            syntax.sexp().contains("integer_literal"),
            "{}",
            syntax.sexp()
        );
        assert!(format!("{syntax:?}").contains("source_file"));
        assert!(Syntax::new("plaintext", "x").is_none());
    }
}
