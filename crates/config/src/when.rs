//! `when` clauses: the context-key expressions that enable a keybinding.
//!
//! The grammar and the token rules follow VS Code's scanner
//! (`src/vs/platform/contextkey/common/scanner.ts:198`) and parser
//! (`src/vs/platform/contextkey/common/contextkey.ts:174`), reimplemented, not copied:
//!
//! ```text
//! expr    := and ('||' and)*
//! and     := term ('&&' term)*
//! term    := '!' (KEY | 'true' | 'false' | '(' expr ')') | primary
//! primary := 'true' | 'false' | '(' expr ')'
//!          | KEY [ '=~' REGEX
//!                | 'not' 'in' value
//!                | ('==' | '!=' | '<' | '<=' | '>' | '>=' | 'in') value ]
//! ```
//!
//! Keybindings are parsed without regex error recovery, as `ContextKeyExpr.deserialize` does
//! (`contextkey.ts:628`).

use std::fmt;

/// A parsed `when` clause.
///
/// The tree keeps the shape of the source, with two simplifications VS Code also makes while
/// parsing: nested `&&` (or `||`) lists are flattened, and `key == true`, `key == false`,
/// `key != true`, `key != false`, `!true` and `!false` become [`When::Has`], [`When::Not`],
/// [`When::True`] or [`When::False`] (`contextkey.ts:262` and `:432`). VS Code goes further and
/// normalizes the whole tree (sorting, removing duplicates, pushing `!` inwards); rustode keeps
/// `!( … )` as [`When::Negate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    /// Always true.
    True,
    /// Always false.
    False,
    /// `key`: the context key is set to a truthy value.
    Has(String),
    /// `!key`.
    Not(String),
    /// `key == value`.
    Equals(String, String),
    /// `key != value`.
    NotEquals(String, String),
    /// `key < value`; the value is compared as a number.
    Smaller(String, String),
    /// `key <= value`.
    SmallerEquals(String, String),
    /// `key > value`.
    Greater(String, String),
    /// `key >= value`.
    GreaterEquals(String, String),
    /// `key =~ /pattern/flags`, with the `g` and `y` flags dropped (`contextkey.ts:525`).
    Regex {
        /// The context key.
        key: String,
        /// The JavaScript regular expression between the slashes, unchanged.
        pattern: String,
        /// The remaining flags (`i`, `s`, `m`, `u`).
        flags: String,
    },
    /// `key in container`: the value of `key` is an element (or a property) of the value of the
    /// context key `container`.
    In {
        /// The context key whose value is looked up.
        key: String,
        /// The context key holding an array or an object.
        container: String,
    },
    /// `key not in container`.
    NotIn {
        /// The context key whose value is looked up.
        key: String,
        /// The context key holding an array or an object.
        container: String,
    },
    /// `a && b && …`.
    And(Vec<When>),
    /// `a || b || …`.
    Or(Vec<When>),
    /// `!( … )`.
    Negate(Box<When>),
}

/// A context key's value while evaluating a `when` clause.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextValue {
    /// The key is not set (`undefined`): falsy.
    Undefined,
    /// A boolean.
    Bool(bool),
    /// A string; empty is falsy.
    Str(String),
    /// A number; zero is falsy.
    Number(f64),
}

impl ContextValue {
    fn truthy(&self) -> bool {
        match self {
            Self::Undefined => false,
            Self::Bool(b) => *b,
            Self::Str(s) => !s.is_empty(),
            Self::Number(n) => *n != 0.0,
        }
    }

    /// JavaScript's loose `==` against a clause value, which VS Code parses
    /// as `true`, `false` or a string.
    fn loose_equals(&self, value: &str) -> bool {
        match (self, value) {
            (Self::Bool(b), "true") => *b,
            (Self::Bool(b), "false") => !*b,
            (Self::Str(s), v) => s == v,
            (Self::Number(n), v) => v.parse::<f64>().is_ok_and(|x| (x - n).abs() < f64::EPSILON),
            _ => false,
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Str(s) => s.parse().ok(),
            _ => None,
        }
    }
}

impl When {
    /// Evaluates the clause with `context` giving each key's value, following
    /// VS Code: a key alone is truthiness (`ContextKeyDefinedExpr`), `==` is
    /// JavaScript's loose equality (`ContextKeyEqualsExpr`), comparisons are
    /// numeric (`src/vs/platform/contextkey/common/contextkey.ts:766-1460`).
    /// Regex matches and `in` are not evaluated yet: `=~` and `in` are false,
    /// `not in` true.
    #[must_use]
    pub fn evaluate(&self, context: &dyn Fn(&str) -> ContextValue) -> bool {
        let compare = |key: &str, value: &str, test: fn(f64, f64) -> bool| match (
            context(key).number(),
            value.parse::<f64>(),
        ) {
            (Some(a), Ok(b)) => test(a, b),
            _ => false,
        };
        match self {
            // Regex matches and `in` are not evaluated yet.
            Self::True | Self::NotIn { .. } => true,
            Self::False | Self::Regex { .. } | Self::In { .. } => false,
            Self::Has(key) => context(key).truthy(),
            Self::Not(key) => !context(key).truthy(),
            Self::Equals(key, value) => context(key).loose_equals(value),
            Self::NotEquals(key, value) => !context(key).loose_equals(value),
            Self::Smaller(key, value) => compare(key, value, |a, b| a < b),
            Self::SmallerEquals(key, value) => compare(key, value, |a, b| a <= b),
            Self::Greater(key, value) => compare(key, value, |a, b| a > b),
            Self::GreaterEquals(key, value) => compare(key, value, |a, b| a >= b),
            Self::And(all) => all.iter().all(|w| w.evaluate(context)),
            Self::Or(any) => any.iter().any(|w| w.evaluate(context)),
            Self::Negate(inner) => !inner.evaluate(context),
        }
    }
}

/// Why a `when` clause did not parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WhenError {
    /// Byte offset in the clause where the problem is.
    pub offset: usize,
    /// Line of the problem in the clause, counting from 1.
    pub line: usize,
    /// Column of the problem in its line, counting chars from 1.
    pub column: usize,
    /// What is wrong, with a hint when VS Code gives one.
    pub message: String,
}

impl fmt::Display for WhenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (line {}, column {})",
            self.message, self.line, self.column
        )
    }
}

impl std::error::Error for WhenError {}

/// Parses a `when` clause.
///
/// # Errors
/// When the clause is empty or is not a valid expression; the error points at the first problem.
pub fn parse_when(input: &str) -> Result<When, WhenError> {
    parse(input).map_err(|mut error| {
        let mut end = error.offset.min(input.len());
        while !input.is_char_boundary(end) {
            end -= 1;
        }
        let before = &input[..end];
        error.line = before.matches('\n').count() + 1;
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        error.column = before[start..].chars().count() + 1;
        error
    })
}

fn parse(input: &str) -> Result<When, WhenError> {
    if input.is_empty() {
        return Err(WhenError {
            offset: 0,
            message: "Empty context key expression. Did you forget to write an expression? You \
                      can also put 'false' or 'true' to always evaluate to false or true."
                .to_owned(),
            ..WhenError::default()
        });
    }
    let tokens = scan(input)?;
    let mut parser = Parser { tokens, current: 0 };
    let expr = parser.or()?;
    let next = parser.peek();
    if next.kind != Tok::Eof {
        let hint = if matches!(next.kind, Tok::Str(_)) {
            " Did you forget to put && or || before the token?"
        } else {
            ""
        };
        return Err(WhenError {
            offset: next.offset,
            message: format!("Unexpected token '{}'.{hint}", next.kind),
            ..WhenError::default()
        });
    }
    Ok(expr)
}

impl fmt::Display for When {
    /// Writes the clause back in a form [`parse_when`] reads as the same tree.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::True => f.write_str("true"),
            Self::False => f.write_str("false"),
            Self::Has(key) => f.write_str(key),
            Self::Not(key) => write!(f, "!{key}"),
            Self::Equals(key, value) => write!(f, "{key} == '{value}'"),
            Self::NotEquals(key, value) => write!(f, "{key} != '{value}'"),
            Self::Smaller(key, value) => write!(f, "{key} < {value}"),
            Self::SmallerEquals(key, value) => write!(f, "{key} <= {value}"),
            Self::Greater(key, value) => write!(f, "{key} > {value}"),
            Self::GreaterEquals(key, value) => write!(f, "{key} >= {value}"),
            Self::Regex {
                key,
                pattern,
                flags,
            } => write!(f, "{key} =~ /{pattern}/{flags}"),
            Self::In { key, container } => write!(f, "{key} in '{container}'"),
            Self::NotIn { key, container } => write!(f, "{key} not in '{container}'"),
            Self::And(items) => write_list(f, items, " && ", |e| matches!(e, Self::Or(_))),
            Self::Or(items) => write_list(f, items, " || ", |e| matches!(e, Self::And(_))),
            Self::Negate(inner) => write!(f, "!({inner})"),
        }
    }
}

fn write_list(
    f: &mut fmt::Formatter<'_>,
    items: &[When],
    separator: &str,
    needs_parens: impl Fn(&When) -> bool,
) -> fmt::Result {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(separator)?;
        }
        if needs_parens(item) {
            write!(f, "({item})")?;
        } else {
            write!(f, "{item}")?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    LParen,
    RParen,
    Neg,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    RegexOp,
    Regex(String),
    True,
    False,
    In,
    Not,
    And,
    Or,
    Str(String),
    Quoted(String),
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::LParen => "(",
            Self::RParen => ")",
            Self::Neg => "!",
            Self::Eq => "==",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::LtEq => "<=",
            Self::Gt => ">",
            Self::GtEq => ">=",
            Self::RegexOp => "=~",
            Self::True => "true",
            Self::False => "false",
            Self::In => "in",
            Self::Not => "not",
            Self::And => "&&",
            Self::Or => "||",
            Self::Eof => "EOF",
            Self::Regex(s) | Self::Str(s) | Self::Quoted(s) => s,
        })
    }
}

#[derive(Debug, Clone)]
struct Token {
    kind: Tok,
    offset: usize,
}

/// Characters of an unquoted key or value: the `stringRe` class of `scanner.ts:302`, where `\p{L}`
/// is approximated by `char::is_alphabetic`. Double quotes are part of the word, as in VS Code.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || "_<>-./\\:*?+[]^,#@;\"%$".contains(c)
        || (!c.is_ascii() && c.is_alphabetic())
}

fn lex_error(offset: usize, lexeme: &str, hint: &str) -> WhenError {
    WhenError {
        offset,
        message: format!("Unexpected token '{lexeme}'. {hint}"),
        ..WhenError::default()
    }
}

/// Splits a clause into tokens (`Scanner.scan`, `scanner.ts:198`).
///
/// VS Code records lexing errors and keeps going; rustode stops at the first one, which VS Code
/// would report anyway.
fn scan(input: &str) -> Result<Vec<Token>, WhenError> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let mut next_is = |expected: char| chars.next_if(|&(_, c)| c == expected).is_some();
        let kind = match c {
            ' ' | '\t' | '\r' | '\n' | '\u{a0}' => continue,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '!' if next_is('=') => {
                next_is('=');
                Tok::NotEq
            }
            '!' => Tok::Neg,
            '=' if next_is('=') => {
                next_is('=');
                Tok::Eq
            }
            '=' if next_is('~') => Tok::RegexOp,
            '=' => return Err(lex_error(start, "=", "Did you mean == or =~?")),
            '<' if next_is('=') => Tok::LtEq,
            '<' => Tok::Lt,
            '>' if next_is('=') => Tok::GtEq,
            '>' => Tok::Gt,
            '&' if next_is('&') => Tok::And,
            '&' => return Err(lex_error(start, "&", "Did you mean &&?")),
            '|' if next_is('|') => Tok::Or,
            '|' => return Err(lex_error(start, "|", "Did you mean ||?")),
            '\'' => {
                // No escapes inside quotes (`scanner.ts:319`).
                let rest = &input[start + 1..];
                let Some(len) = rest.find('\'') else {
                    return Err(lex_error(
                        start,
                        &input[start..],
                        "Did you forget to open or close the quote?",
                    ));
                };
                // Skip the text and the closing quote.
                while chars.next_if(|&(i, _)| i <= start + 1 + len).is_some() {}
                tokens.push(Token {
                    kind: Tok::Quoted(rest[..len].to_owned()),
                    offset: start + 1,
                });
                continue;
            }
            '/' => {
                let end = regex_end(input, start).ok_or_else(|| {
                    lex_error(
                        start,
                        &input[start..],
                        "Did you forget to escape the '/' (slash) character? Put two \
                         backslashes before it to escape, e.g., '\\\\/'.",
                    )
                })?;
                while chars.next_if(|&(i, _)| i < end).is_some() {}
                Tok::Regex(input[start..end].to_owned())
            }
            c if is_word_char(c) => {
                let mut end = start + c.len_utf8();
                while let Some((i, c)) = chars.next_if(|&(_, c)| is_word_char(c)) {
                    end = i + c.len_utf8();
                }
                match &input[start..end] {
                    "not" => Tok::Not,
                    "in" => Tok::In,
                    "false" => Tok::False,
                    "true" => Tok::True,
                    word => Tok::Str(word.to_owned()),
                }
            }
            // VS Code silently drops a character it cannot scan; rustode reports it.
            c => return Err(lex_error(start, &c.to_string(), "")),
        };
        tokens.push(Token {
            kind,
            offset: start,
        });
    }
    tokens.push(Token {
        kind: Tok::Eof,
        offset: input.len(),
    });
    Ok(tokens)
}

/// The end of a `/…/flags` literal that starts at `start` (`scanner.ts:341`): a `/` inside a
/// character class or after a backslash does not close it.
fn regex_end(input: &str, start: usize) -> Option<usize> {
    let mut in_escape = false;
    let mut in_class = false;
    let mut body_end = None;
    for (i, c) in input[start + 1..].char_indices() {
        if in_escape {
            in_escape = false;
        } else if c == '/' && !in_class {
            body_end = Some(start + 1 + i + 1);
            break;
        } else if c == '[' {
            in_class = true;
        } else if c == '\\' {
            in_escape = true;
        } else if c == ']' {
            in_class = false;
        }
    }
    let mut end = body_end?;
    while input[end..].starts_with(['i', 'g', 's', 'm', 'y', 'u']) {
        end += 1;
    }
    Some(end)
}

struct Parser {
    tokens: Vec<Token>,
    current: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.current].clone();
        if token.kind != Tok::Eof {
            self.current += 1;
        }
        token
    }

    fn eat(&mut self, kind: &Tok) -> bool {
        if &self.peek().kind == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expected(&self, what: &str) -> WhenError {
        let got = self.peek();
        WhenError {
            offset: got.offset,
            message: format!("Expected: {what}. Received: '{}'.", got.kind),
            ..WhenError::default()
        }
    }

    fn expect(&mut self, kind: &Tok, what: &str) -> Result<(), WhenError> {
        if self.eat(kind) {
            Ok(())
        } else {
            Err(self.expected(what))
        }
    }

    /// `_or` (`contextkey.ts:239`).
    fn or(&mut self) -> Result<When, WhenError> {
        let mut items = vec![self.and()?];
        while self.eat(&Tok::Or) {
            items.push(self.and()?);
        }
        Ok(flatten(
            items,
            |e| match e {
                When::Or(inner) => Ok(inner),
                other => Err(other),
            },
            When::Or,
        ))
    }

    /// `_and` (`contextkey.ts:250`).
    fn and(&mut self) -> Result<When, WhenError> {
        let mut items = vec![self.term()?];
        while self.eat(&Tok::And) {
            items.push(self.term()?);
        }
        Ok(flatten(
            items,
            |e| match e {
                When::And(inner) => Ok(inner),
                other => Err(other),
            },
            When::And,
        ))
    }

    /// `_term` (`contextkey.ts:261`).
    fn term(&mut self) -> Result<When, WhenError> {
        if !self.eat(&Tok::Neg) {
            return self.primary();
        }
        match self.peek().kind.clone() {
            Tok::True => {
                self.advance();
                Ok(When::False)
            }
            Tok::False => {
                self.advance();
                Ok(When::True)
            }
            Tok::LParen => {
                self.advance();
                let inner = self.or()?;
                self.expect(&Tok::RParen, "closing parenthesis ')'")?;
                Ok(When::Negate(Box::new(inner)))
            }
            Tok::Str(key) => {
                self.advance();
                Ok(When::Not(key))
            }
            _ => Err(self.expected("KEY | true | false | '(' expression ')'")),
        }
    }

    /// `_primary` (`contextkey.ts:287`).
    fn primary(&mut self) -> Result<When, WhenError> {
        match self.peek().kind.clone() {
            Tok::True => {
                self.advance();
                Ok(When::True)
            }
            Tok::False => {
                self.advance();
                Ok(When::False)
            }
            Tok::LParen => {
                self.advance();
                let inner = self.or()?;
                self.expect(&Tok::RParen, "closing parenthesis ')'")?;
                Ok(inner)
            }
            Tok::Str(key) => {
                self.advance();
                self.after_key(key)
            }
            Tok::Eof => Err(WhenError {
                offset: self.peek().offset,
                message: "Unexpected end of expression. Did you forget to put a context key?"
                    .to_owned(),
                ..WhenError::default()
            }),
            _ => Err(self.expected(
                "true | false | KEY | KEY '=~' REGEX | KEY ('==' | '!=' | '<' | '<=' | '>' | \
                 '>=' | 'in' | 'not' 'in') value",
            )),
        }
    }

    fn after_key(&mut self, key: String) -> Result<When, WhenError> {
        if self.eat(&Tok::RegexOp) {
            let Tok::Regex(lexeme) = self.peek().kind.clone() else {
                return Err(self.expected("REGEX"));
            };
            self.advance();
            let close = lexeme.rfind('/').unwrap_or(0);
            return Ok(When::Regex {
                key,
                pattern: lexeme[1..close].to_owned(),
                flags: lexeme[close + 1..].replace(['g', 'y'], ""),
            });
        }
        if self.eat(&Tok::Not) {
            self.expect(&Tok::In, "'in' after 'not'")?;
            let (container, _) = self.value();
            return Ok(When::NotIn { key, container });
        }
        let operator = self.peek().kind.clone();
        let binary = match operator {
            Tok::Eq | Tok::NotEq | Tok::Lt | Tok::LtEq | Tok::Gt | Tok::GtEq | Tok::In => {
                self.advance();
                self.value()
            }
            _ => return Ok(When::Has(key)),
        };
        let (value, quoted) = binary;
        Ok(match (operator, quoted, value.as_str()) {
            (Tok::Eq, false, "true") | (Tok::NotEq, false, "false") => When::Has(key),
            (Tok::Eq, false, "false") | (Tok::NotEq, false, "true") => When::Not(key),
            (Tok::Eq, ..) => When::Equals(key, value),
            (Tok::NotEq, ..) => When::NotEquals(key, value),
            (Tok::Lt, ..) => When::Smaller(key, value),
            (Tok::LtEq, ..) => When::SmallerEquals(key, value),
            (Tok::Gt, ..) => When::Greater(key, value),
            (Tok::GtEq, ..) => When::GreaterEquals(key, value),
            _ => When::In {
                key,
                container: value,
            },
        })
    }

    /// `_value` (`contextkey.ts:502`): a missing value is the empty string, as existing extensions
    /// rely on `"foo == "`. Returns whether the value was quoted.
    fn value(&mut self) -> (String, bool) {
        match self.peek().kind.clone() {
            Tok::Str(s) => {
                self.advance();
                (s, false)
            }
            Tok::Quoted(s) => {
                self.advance();
                (s, true)
            }
            Tok::True => {
                self.advance();
                ("true".to_owned(), false)
            }
            Tok::False => {
                self.advance();
                ("false".to_owned(), false)
            }
            Tok::In => {
                self.advance();
                ("in".to_owned(), false)
            }
            _ => (String::new(), false),
        }
    }
}

/// Builds an n-ary node, splicing in the items of nested nodes of the same kind.
fn flatten(
    items: Vec<When>,
    split: impl Fn(When) -> Result<Vec<When>, When>,
    build: impl Fn(Vec<When>) -> When,
) -> When {
    if items.len() == 1 {
        return items.into_iter().next().unwrap_or(When::True);
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match split(item) {
            Ok(inner) => out.extend(inner),
            Err(item) => out.push(item),
        }
    }
    build(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(key: &str) -> When {
        When::Has(key.to_owned())
    }

    fn ok(input: &str) -> When {
        parse_when(input).unwrap_or_else(|e| panic!("{input}: {e}"))
    }

    fn err(input: &str) -> usize {
        match parse_when(input) {
            Ok(w) => panic!("{input} parsed as {w:?}"),
            Err(e) => e.offset,
        }
    }

    #[test]
    fn errors_name_a_line_and_column_not_a_byte_offset() {
        let error = parse_when("é && )").expect_err("an error");
        assert_eq!((error.line, error.column), (1, 6));
        assert!(error.to_string().ends_with("(line 1, column 6)"), "{error}");
        let error = parse_when("a &&\n&& b").expect_err("an error");
        assert_eq!((error.line, error.column), (2, 1));
    }

    #[test]
    fn precedence_and_parentheses() {
        assert_eq!(
            ok("a || b && c"),
            When::Or(vec![has("a"), When::And(vec![has("b"), has("c")])])
        );
        assert_eq!(
            ok("(a || b) && c"),
            When::And(vec![When::Or(vec![has("a"), has("b")]), has("c")])
        );
        assert_eq!(
            ok("a && (b && c)"),
            When::And(vec![has("a"), has("b"), has("c")])
        );
        assert_eq!(ok("((a))"), has("a"));
    }

    #[test]
    fn negation() {
        assert_eq!(ok("!a"), When::Not("a".into()));
        assert_eq!(ok("!true"), When::False);
        assert_eq!(ok("!false"), When::True);
        assert_eq!(
            ok("!(a || b)"),
            When::Negate(Box::new(When::Or(vec![has("a"), has("b")])))
        );
        assert_eq!(err("!!a"), 1);
        assert_eq!(err("!a == b"), 3, "! binds to the key only");
    }

    #[test]
    fn comparisons() {
        assert_eq!(
            ok("editorLangId == rust"),
            When::Equals("editorLangId".into(), "rust".into())
        );
        assert_eq!(ok("a === 'x y'"), When::Equals("a".into(), "x y".into()));
        assert_eq!(ok("a != b"), When::NotEquals("a".into(), "b".into()));
        assert_eq!(ok("a !== b"), When::NotEquals("a".into(), "b".into()));
        assert_eq!(ok("a == true"), has("a"));
        assert_eq!(ok("a == false"), When::Not("a".into()));
        assert_eq!(ok("a != true"), When::Not("a".into()));
        assert_eq!(ok("a != false"), has("a"));
        assert_eq!(ok("a == 'true'"), When::Equals("a".into(), "true".into()));
        assert_eq!(ok("a == "), When::Equals("a".into(), String::new()));
        assert_eq!(ok("a == in"), When::Equals("a".into(), "in".into()));
    }

    #[test]
    fn ordering_comparisons() {
        assert_eq!(ok("count < 3"), When::Smaller("count".into(), "3".into()));
        assert_eq!(
            ok("count <= 3"),
            When::SmallerEquals("count".into(), "3".into())
        );
        assert_eq!(ok("count > 3"), When::Greater("count".into(), "3".into()));
        assert_eq!(
            ok("count >= 3"),
            When::GreaterEquals("count".into(), "3".into())
        );
        assert_eq!(
            ok("config.editor.tabSize==2"),
            When::Equals("config.editor.tabSize".into(), "2".into())
        );
    }

    #[test]
    fn in_and_not_in() {
        assert_eq!(
            ok("resourceFilename in supportedFiles"),
            When::In {
                key: "resourceFilename".into(),
                container: "supportedFiles".into()
            }
        );
        assert_eq!(
            ok("a not in 'b'"),
            When::NotIn {
                key: "a".into(),
                container: "b".into()
            }
        );
        assert_eq!(err("a not b"), 6);
    }

    #[test]
    fn regexes() {
        assert_eq!(
            ok("resourceFilename =~ /docker/"),
            When::Regex {
                key: "resourceFilename".into(),
                pattern: "docker".into(),
                flags: String::new()
            }
        );
        assert_eq!(
            ok("a =~ /x\\/y[/]/gimy"),
            When::Regex {
                key: "a".into(),
                pattern: "x\\/y[/]".into(),
                flags: "im".into()
            }
        );
        assert_eq!(
            err("a =~ 'x'"),
            6,
            "quoted regexes are refused without recovery"
        );
        assert_eq!(err("a =~ /open"), 5);
    }

    #[test]
    fn errors_point_at_the_problem() {
        assert_eq!(err(""), 0);
        assert_eq!(err("a &&"), 4);
        assert_eq!(err("a & b"), 2);
        assert_eq!(err("a | b"), 2);
        assert_eq!(err("a = b"), 2);
        assert_eq!(err("a b"), 2);
        assert_eq!(err("(a"), 2);
        assert_eq!(err("a)"), 1);
        assert_eq!(err("a == 'b"), 5);
        assert_eq!(err("a ~ b"), 2);
    }

    #[test]
    fn keys_with_punctuation_and_letters() {
        assert_eq!(
            ok("view == workbench.view.explorer"),
            When::Equals("view".into(), "workbench.view.explorer".into())
        );
        assert_eq!(
            ok("resourceScheme==vscode-userdata"),
            When::Equals("resourceScheme".into(), "vscode-userdata".into())
        );
        assert_eq!(ok("clé"), has("clé"));
    }

    #[test]
    fn display_round_trips() {
        for input in [
            "a && !b || c == 'x' && d =~ /y/i",
            "!(a || b) && e in 'f' && g not in 'h'",
            "(a || b) && (c || d)",
            "n < 1 || n <= 2 || n > 3 || n >= 4 || m != 'q' || true || false",
        ] {
            let parsed = ok(input);
            assert_eq!(ok(&parsed.to_string()), parsed, "{input} -> {parsed}");
        }
    }

    #[test]
    fn clauses_evaluate_like_vs_code() {
        let context = |key: &str| match key {
            "editorTextFocus" => ContextValue::Bool(true),
            "editorLangId" => ContextValue::Str("rust".into()),
            "count" => ContextValue::Number(2.0),
            _ => ContextValue::Undefined,
        };
        let eval = |clause: &str| parse_when(clause).expect("parses").evaluate(&context);
        assert!(eval("editorTextFocus && editorLangId == rust"));
        assert!(!eval("editorTextFocus && !editorTextFocus"));
        assert!(eval("missing || editorTextFocus == true"));
        assert!(eval("editorLangId != python && count > 1 && count <= 2"));
        assert!(!eval("missing"));
    }
}
