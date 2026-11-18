//! The grammars rustode can parse, and how their captures become token
//! kinds. Only Rust so far (ticket 28 adds more).
//!
//! Rust uses the `tree-sitter-rust` grammar and its highlight query
//! (`tree_sitter_rust::HIGHLIGHTS_QUERY`, MIT), with a few patterns of
//! rustode's own around it:
//!
//! - before it, so that it wins, all-caps names are constants: the
//!   grammar's own pattern for them has a stray `'` in its regex and never
//!   matches (`queries/highlights.scm:9-11` of tree-sitter-rust 0.24.2);
//! - after it, so that everything in it wins, path segments are namespaces
//!   and any other name a variable, as VS Code's Rust grammar colors them
//!   (`entity.name.namespace.rust` and `variable.other.rust`,
//!   `extensions/rust/syntaxes/rust.tmLanguage.json:915-929`, `:1188-1194`).
//!   VS Code's own tree-sitter queries end the same way, with `(identifier)`
//!   as their least specific pattern
//!   (`src/vs/editor/common/languages/highlights/typescript.scm:1-5`).
//!
//! Some captures cover constructs that VS Code's Rust grammar splits, so
//! the node refines them: control flow keywords are `keyword.control.rust`
//! and `macro_rules!` a macro name (`rust.tmLanguage.json:683-684`, `:109`),
//! `true` and `false` are not numbers (`:381`), `::`, `:` and `.` are
//! operators while `,` and `;` are punctuation (`:800`, `:829`, `:819`,
//! `:1064`, `:1079`), and a lifetime's `'` is punctuation (`:874`).

use crate::TokenKind;
use std::sync::OnceLock;
use tree_sitter::{Language, Query};

/// rustode's patterns that take precedence over the grammar's query.
const RUST_BEFORE: &str = r#"
((identifier) @constant
 (#match? @constant "^[A-Z][A-Z\\d_]+$"))
"#;

/// rustode's patterns that every pattern of the grammar's query overrides.
const RUST_AFTER: &str = r"
(scoped_identifier path: (identifier) @namespace)
(scoped_identifier path: (scoped_identifier name: (identifier) @namespace))
(scoped_type_identifier path: (identifier) @namespace)
(scoped_type_identifier path: (scoped_identifier name: (identifier) @namespace))
(scoped_use_list path: (identifier) @namespace)
(identifier) @variable
";

/// Keywords VS Code's Rust grammar scopes as `keyword.control.rust`
/// (`rust.tmLanguage.json:683-684`).
const CONTROL_KEYWORDS: &[&str] = &[
    "await", "break", "continue", "do", "else", "for", "if", "loop", "match", "return", "try",
    "while", "yield",
];

/// What a capture name says, before the captured node refines it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Always this kind.
    Fixed(TokenKind),
    /// A keyword: control flow, `macro_rules!` or another one.
    Keyword,
    /// `::`, `:`, `.`, `,` or `;`.
    Delimiter,
    /// `*`, `&` or a lifetime's `'`.
    Operator,
    /// A boolean or a number.
    Literal,
}

impl Class {
    fn of(capture: &str) -> Option<Self> {
        use TokenKind as K;
        Some(match capture {
            "comment" | "comment.documentation" => Self::Fixed(K::Comment),
            "string" => Self::Fixed(K::String),
            "escape" => Self::Fixed(K::Escape),
            "constant" => Self::Fixed(K::Constant),
            "constant.builtin" => Self::Literal,
            "keyword" => Self::Keyword,
            "variable.builtin" => Self::Fixed(K::Keyword),
            "function" | "function.method" | "function.macro" => Self::Fixed(K::Function),
            "type" | "type.builtin" | "constructor" | "label" | "namespace" => Self::Fixed(K::Type),
            "property" | "variable" | "variable.parameter" => Self::Fixed(K::Variable),
            "punctuation.bracket" => Self::Fixed(K::Punctuation),
            "punctuation.delimiter" => Self::Delimiter,
            "operator" => Self::Operator,
            "attribute" => Self::Fixed(K::Attribute),
            _ => return None,
        })
    }

    /// The kind of a node of type `node` captured with this class.
    fn kind(self, node: &str) -> TokenKind {
        match self {
            Self::Fixed(kind) => kind,
            Self::Keyword if CONTROL_KEYWORDS.contains(&node) => TokenKind::ControlKeyword,
            Self::Keyword if node == "macro_rules!" => TokenKind::Function,
            Self::Keyword => TokenKind::Keyword,
            Self::Delimiter if matches!(node, "::" | ":" | ".") => TokenKind::Operator,
            Self::Operator if node != "'" => TokenKind::Operator,
            Self::Delimiter | Self::Operator => TokenKind::Punctuation,
            Self::Literal if node == "boolean_literal" => TokenKind::Boolean,
            Self::Literal => TokenKind::Number,
        }
    }
}

/// A language's parser table and highlight query.
pub(crate) struct Grammar {
    pub(crate) language: Language,
    pub(crate) query: Query,
    /// The class of each of the query's captures, by capture index.
    classes: Vec<Option<Class>>,
}

impl Grammar {
    /// The grammar for a language id (`rust`), if rustode has one.
    pub(crate) fn for_language(language_id: &str) -> Option<&'static Self> {
        static RUST: OnceLock<Option<Grammar>> = OnceLock::new();
        match language_id {
            "rust" => RUST.get_or_init(Self::rust).as_ref(),
            _ => None,
        }
    }

    fn rust() -> Option<Self> {
        let language = Language::new(tree_sitter_rust::LANGUAGE);
        let source = [RUST_BEFORE, tree_sitter_rust::HIGHLIGHTS_QUERY, RUST_AFTER].concat();
        let query = Query::new(&language, &source).ok()?;
        let classes = query.capture_names().iter().map(|n| Class::of(n)).collect();
        Some(Self {
            language,
            query,
            classes,
        })
    }

    /// The kind of `node_kind` captured as capture `index`, if it is a
    /// highlight capture.
    pub(crate) fn kind(&self, index: u32, node_kind: &str) -> Option<TokenKind> {
        let class = self.classes.get(usize::try_from(index).ok()?).copied()??;
        Some(class.kind(node_kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rust_query_compiles_and_every_capture_has_a_kind() {
        let grammar = Grammar::for_language("rust").expect("rust grammar");
        for (index, name) in grammar.query.capture_names().iter().enumerate() {
            assert!(
                Class::of(name).is_some(),
                "capture @{name} ({index}) has no kind"
            );
        }
        assert!(Grammar::for_language("plaintext").is_none());
    }

    #[test]
    fn nodes_refine_keywords_delimiters_operators_and_literals() {
        assert_eq!(Class::Keyword.kind("match"), TokenKind::ControlKeyword);
        assert_eq!(Class::Keyword.kind("fn"), TokenKind::Keyword);
        assert_eq!(Class::Keyword.kind("macro_rules!"), TokenKind::Function);
        assert_eq!(Class::Delimiter.kind("::"), TokenKind::Operator);
        assert_eq!(Class::Delimiter.kind(";"), TokenKind::Punctuation);
        assert_eq!(Class::Operator.kind("&"), TokenKind::Operator);
        assert_eq!(Class::Operator.kind("'"), TokenKind::Punctuation);
        assert_eq!(Class::Literal.kind("boolean_literal"), TokenKind::Boolean);
        assert_eq!(Class::Literal.kind("float_literal"), TokenKind::Number);
    }
}
