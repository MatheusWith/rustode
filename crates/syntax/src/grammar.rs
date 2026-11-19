//! The grammars rustode can parse, and how their captures become token
//! kinds: Rust, JSON and JSON with Comments, TOML, Markdown, JavaScript and
//! TypeScript (with their JSX forms), Python, C, C++, Go, shell scripts,
//! YAML, HTML and CSS (ticket 28; ticket 27 built the Rust one).
//!
//! Every grammar is a `tree-sitter-*` crate (MIT) with its own highlight
//! query, mapped onto the same [`TokenKind`]s and Dark Modern colors by
//! capture name ([`Class::of`]) and, for keywords and literals, by node.
//! The kinds stand for the `TextMate` scopes of VS Code's grammars, listed in
//! the [`token`](crate::token) docs; where a language has a construct VS
//! Code colors in a way no kind has, the closest kind is used and the
//! difference is noted below.
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

/// Keywords VS Code's grammars scope as `keyword.control`
/// (`rust.tmLanguage.json:683-684`, and the same set in the other languages'
/// grammars, e.g. `extensions/javascript/syntaxes/JavaScript.tmLanguage.json`
/// `keyword.control.*` and `extensions/python/syntaxes/MagicPython.tmLanguage.json`
/// `keyword.control.flow.python`).
const CONTROL_KEYWORDS: &[&str] = &[
    "await", "break", "case", "catch", "continue", "default", "defer", "do", "elif", "else",
    "except", "fi", "finally", "for", "go", "goto", "if", "loop", "match", "raise", "return",
    "select", "switch", "then", "throw", "try", "while", "with", "yield",
];

/// Nodes that are `constant.language` in VS Code's grammars: booleans and
/// the null values (`true`, `false`, `null`, `nil`, `None`, `undefined`).
const LANGUAGE_CONSTANTS: &[&str] = &[
    "boolean",
    "boolean_literal",
    "false",
    "nil",
    "none",
    "null_scalar",
    "null",
    "true",
    "undefined",
];

/// TOML: the grammar's highlight query (`tree-sitter-toml-ng` 0.7.0,
/// `queries/highlights.scm`), but with keys a property name and no pattern
/// that colors a whole `key = value` pair: VS Code's TOML grammar
/// (`support.type.property-name`, `dark_vs.json:325-336`) colors the key
/// only.
const TOML_QUERY: &str = r#"
(bare_key) @property
(quoted_key) @property
(boolean) @boolean
(comment) @comment
(string) @string
[(integer) (float)] @number
[
  (offset_date_time)
  (local_date_time)
  (local_date)
  (local_time)
] @string.special
["." ","] @punctuation.delimiter
"=" @operator
["[" "]" "[[" "]]" "{" "}"] @punctuation.bracket
"#;

/// YAML mapping keys, before the grammar's query: its `@string` pattern for
/// every scalar would otherwise win over the later key patterns. VS Code
/// colors keys as `entity.name.tag.yaml` (`#569CD6`,
/// `extensions/yaml/syntaxes/yaml.tmLanguage.json:55`, `dark_vs.json:108-113`);
/// the nearest kind with that color is a keyword.
const YAML_KEYS: &str = r"
(block_mapping_pair key: (flow_node [(double_quote_scalar) (single_quote_scalar)] @keyword))
(block_mapping_pair key: (flow_node (plain_scalar (string_scalar) @keyword)))
(flow_mapping (_ key: (flow_node [(double_quote_scalar) (single_quote_scalar)] @keyword)))
(flow_mapping (_ key: (flow_node (plain_scalar (string_scalar) @keyword))))
";

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
    /// A boolean, a null value or a number.
    Literal,
}

impl Class {
    /// The class of capture `capture`, before a grammar's own overrides.
    fn of(capture: &str) -> Option<Self> {
        use TokenKind as K;
        Some(match capture {
            "comment" | "comment.documentation" => Self::Fixed(K::Comment),
            "string" | "string.special" | "text.literal" | "text.uri" | "text.reference" => {
                Self::Fixed(K::String)
            }
            "escape" | "string.escape" => Self::Fixed(K::Escape),
            "constant" => Self::Fixed(K::Constant),
            "constant.builtin" => Self::Literal,
            "number" => Self::Fixed(K::Number),
            "boolean" => Self::Fixed(K::Boolean),
            "keyword" | "text.title" | "tag" | "punctuation.special.keyword" => Self::Keyword,
            "variable.builtin" => Self::Fixed(K::Keyword),
            "function" | "function.method" | "function.macro" | "function.builtin"
            | "function.special" => Self::Fixed(K::Function),
            "type" | "type.builtin" | "constructor" | "label" | "namespace" => Self::Fixed(K::Type),
            "property" | "variable" | "variable.parameter" | "string.special.key" => {
                Self::Fixed(K::Variable)
            }
            "punctuation.bracket" | "punctuation.special" => Self::Fixed(K::Punctuation),
            "punctuation.delimiter" | "delimiter" => Self::Delimiter,
            "operator" => Self::Operator,
            "attribute" => Self::Fixed(K::Variable),
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
            Self::Literal if LANGUAGE_CONSTANTS.contains(&node) => TokenKind::Boolean,
            Self::Literal => TokenKind::Number,
        }
    }
}

/// The ids of the languages that have a grammar, in the order of
/// [`CELLS`].
const IDS: [&str; 17] = [
    "rust",
    "json",
    "jsonc",
    "toml",
    "markdown",
    "javascript",
    "javascriptreact",
    "typescript",
    "typescriptreact",
    "python",
    "c",
    "cpp",
    "go",
    "shellscript",
    "yaml",
    "html",
    "css",
];

/// Each grammar, built the first time a document needs it.
static CELLS: [OnceLock<Option<Grammar>>; IDS.len()] = [const { OnceLock::new() }; IDS.len()];

/// A language's parser table and highlight query.
pub(crate) struct Grammar {
    pub(crate) language: Language,
    pub(crate) query: Query,
    /// The class of each of the query's captures, by capture index.
    classes: Vec<Option<Class>>,
}

/// A grammar's parser and the highlight queries to concatenate, the
/// patterns that must win first.
struct Source {
    language: Language,
    queries: Vec<String>,
    /// Captures this grammar colors differently from [`Class::of`].
    overrides: Vec<(&'static str, TokenKind)>,
}

impl Source {
    fn one(language: impl Into<Language>, query: &str) -> Self {
        Self::new(language, &[query])
    }

    fn new(language: impl Into<Language>, queries: &[&str]) -> Self {
        Self {
            language: language.into(),
            queries: queries.iter().map(|q| (*q).to_owned()).collect(),
            overrides: Vec::new(),
        }
    }
}

/// The queries joined, with every catch-all `(identifier) @variable` line
/// moved to the end: the JavaScript, Python and C queries start with it
/// and are written for a highlighter where the last pattern wins, while
/// rustode's picks the first (see the crate docs). Rust's own queries
/// already end that way.
fn defer_generic(queries: &[String]) -> String {
    let generic = "(identifier) @variable";
    let mut joined = String::new();
    let mut deferred = false;
    for line in queries.iter().flat_map(|q| q.lines()) {
        if line.trim() == generic {
            deferred = true;
        } else {
            joined.push_str(line);
            joined.push('\n');
        }
    }
    if deferred {
        joined.push_str(generic);
        joined.push('\n');
    }
    joined
}

/// The JavaScript query, with TypeScript's patterns first when it is
/// TypeScript, so that its keywords and types win, and JSX's when the
/// grammar has JSX.
fn javascript_query(extra: &[&str], jsx: bool) -> Vec<String> {
    let mut queries: Vec<String> = extra.iter().map(|q| (*q).to_owned()).collect();
    queries.push(tree_sitter_javascript::HIGHLIGHT_QUERY.to_owned());
    if jsx {
        queries.push(tree_sitter_javascript::JSX_HIGHLIGHT_QUERY.to_owned());
    }
    queries
}

impl Grammar {
    /// The grammar for a language id (`rust`), if rustode has one.
    pub(crate) fn for_language(language_id: &str) -> Option<&'static Self> {
        let index = IDS.iter().position(|id| *id == language_id)?;
        CELLS[index]
            .get_or_init(|| Self::build(Self::source(language_id)?))
            .as_ref()
    }

    fn source(language_id: &str) -> Option<Source> {
        Some(match language_id {
            "rust" => Self::rust_source(),
            "json" | "jsonc" => Source::one(
                tree_sitter_json::LANGUAGE,
                tree_sitter_json::HIGHLIGHTS_QUERY,
            ),
            "toml" => Source::one(tree_sitter_toml_ng::LANGUAGE, TOML_QUERY),
            "markdown" => Source::one(
                tree_sitter_md::LANGUAGE,
                tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
            ),
            "javascript" | "javascriptreact" => Source {
                queries: javascript_query(&[], true),
                ..Source::new(tree_sitter_javascript::LANGUAGE, &[])
            },
            "typescript" => Source {
                queries: javascript_query(&[tree_sitter_typescript::HIGHLIGHTS_QUERY], false),
                ..Source::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT, &[])
            },
            "typescriptreact" => Source {
                queries: javascript_query(&[tree_sitter_typescript::HIGHLIGHTS_QUERY], true),
                ..Source::new(tree_sitter_typescript::LANGUAGE_TSX, &[])
            },
            "python" => Source::one(
                tree_sitter_python::LANGUAGE,
                tree_sitter_python::HIGHLIGHTS_QUERY,
            ),
            "c" => Source::one(tree_sitter_c::LANGUAGE, tree_sitter_c::HIGHLIGHT_QUERY),
            // C++ extends C: its own patterns first, then C's.
            "cpp" => Source::new(
                tree_sitter_cpp::LANGUAGE,
                &[
                    tree_sitter_cpp::HIGHLIGHT_QUERY,
                    tree_sitter_c::HIGHLIGHT_QUERY,
                ],
            ),
            "go" => Source::one(tree_sitter_go::LANGUAGE, tree_sitter_go::HIGHLIGHTS_QUERY),
            "shellscript" => Source::one(
                tree_sitter_bash::LANGUAGE,
                tree_sitter_bash::HIGHLIGHT_QUERY,
            ),
            "yaml" => Source {
                overrides: vec![("attribute", TokenKind::Type)],
                queries: vec![
                    YAML_KEYS.to_owned(),
                    tree_sitter_yaml::HIGHLIGHTS_QUERY.to_owned(),
                ],
                ..Source::new(tree_sitter_yaml::LANGUAGE, &[])
            },
            "html" => Source::one(
                tree_sitter_html::LANGUAGE,
                tree_sitter_html::HIGHLIGHTS_QUERY,
            ),
            "css" => Source {
                // `1px`: the unit is `keyword.other.unit.css`, a number's color
                // (`extensions/theme-defaults/themes/dark_vs.json:375-379`).
                overrides: vec![("type", TokenKind::Number)],
                ..Source::one(tree_sitter_css::LANGUAGE, tree_sitter_css::HIGHLIGHTS_QUERY)
            },
            _ => return None,
        })
    }

    fn rust_source() -> Source {
        Source {
            language: tree_sitter_rust::LANGUAGE.into(),
            queries: vec![
                RUST_BEFORE.to_owned(),
                tree_sitter_rust::HIGHLIGHTS_QUERY.to_owned(),
                RUST_AFTER.to_owned(),
            ],
            overrides: vec![("attribute", TokenKind::Attribute)],
        }
    }

    fn build(source: Source) -> Option<Self> {
        let query = Query::new(&source.language, &defer_generic(&source.queries)).ok()?;
        let classes = query
            .capture_names()
            .iter()
            .map(
                |n| match source.overrides.iter().find(|(name, _)| name == n) {
                    Some((_, kind)) => Some(Class::Fixed(*kind)),
                    None => Class::of(n),
                },
            )
            .collect();
        Some(Self {
            language: source.language,
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

    /// Captures that are deliberately plain: text that another grammar
    /// would color (embedded code, the content of a code fence) or an
    /// erroneous tag.
    const PLAIN: &[&str] = &["embedded", "none", "tag.error"];

    #[test]
    fn every_query_compiles_and_every_capture_has_a_kind() {
        for id in IDS {
            let grammar = Grammar::for_language(id).unwrap_or_else(|| panic!("{id} grammar"));
            for (index, name) in grammar.query.capture_names().iter().enumerate() {
                assert!(
                    PLAIN.contains(name) || grammar.classes[index].is_some(),
                    "{id}: capture @{name} ({index}) has no kind"
                );
            }
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
        assert_eq!(Class::Literal.kind("null"), TokenKind::Boolean);
        assert_eq!(Class::Literal.kind("float_literal"), TokenKind::Number);
    }
}
