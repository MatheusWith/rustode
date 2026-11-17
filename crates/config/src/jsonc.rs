//! JSON with comments and trailing commas, parsed with error recovery and byte spans.
//!
//! The behavior follows VS Code's scanner and visitor in `src/vs/base/common/json.ts`
//! (reimplemented, not copied):
//! - `//` and `/* */` comments are trivia (`json.ts:429`);
//! - a comma before `}` or `]` is accepted (`allowTrailingComma`, `json.ts:1246` and `:1278`),
//!   which is the default for both settings and keybindings (`json.ts:145`);
//! - after an error the parser skips to the next `,`, `}` or `]` and goes on, so one typo never
//!   hides the rest of the file (`handleError`, `json.ts:1156`);
//! - an empty document is not an error (`allowEmptyContent`, `json.ts:1313`).

use std::collections::BTreeMap;
use std::fmt;

/// Nesting deeper than this is reported instead of risking the stack. VS Code has no limit.
const MAX_DEPTH: usize = 256;

/// A byte range in the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

/// A JSON value without positions.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// Any number; JSON numbers are doubles, as in JavaScript.
    Number(f64),
    /// A string, with escapes decoded.
    String(String),
    /// An array.
    Array(Vec<Value>),
    /// An object; for a repeated key the last value wins, as in `JSON.parse`.
    Object(BTreeMap<String, Value>),
}

impl Value {
    /// The JSON type name used in messages: `null`, `boolean`, `number`, `string`, `array` or
    /// `object`.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }
}

/// A parsed value with its span.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Where the value is in the source.
    pub span: Span,
    /// The value and its children.
    pub kind: NodeKind,
}

/// The shape of a [`Node`].
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Node>),
    /// An object's properties in source order, repeated keys included.
    Object(Vec<Property>),
}

/// One `"key": value` pair of an object.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    /// The decoded key.
    pub key: String,
    /// Where the key (with its quotes) is.
    pub key_span: Span,
    /// The value, or `None` when it is missing or malformed.
    pub value: Option<Node>,
}

impl Node {
    /// The value without positions; properties without a value are left out.
    #[must_use]
    pub fn to_value(&self) -> Value {
        match &self.kind {
            NodeKind::Null => Value::Null,
            NodeKind::Bool(b) => Value::Bool(*b),
            NodeKind::Number(n) => Value::Number(*n),
            NodeKind::String(s) => Value::String(s.clone()),
            NodeKind::Array(items) => Value::Array(items.iter().map(Node::to_value).collect()),
            NodeKind::Object(props) => Value::Object(
                props
                    .iter()
                    .filter_map(|p| Some((p.key.clone(), p.value.as_ref()?.to_value())))
                    .collect(),
            ),
        }
    }
}

/// A syntax error, named after VS Code's `ParseErrorCode` (`json.ts:82`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonError {
    /// A character that cannot start a token.
    InvalidSymbol,
    /// A number that is not valid JSON.
    InvalidNumberFormat,
    /// An object member does not start with a string key.
    PropertyNameExpected,
    /// A value is missing.
    ValueExpected,
    /// A key is not followed by `:`.
    ColonExpected,
    /// Two members or elements are not separated by `,`.
    CommaExpected,
    /// An object is not closed.
    CloseBraceExpected,
    /// An array is not closed.
    CloseBracketExpected,
    /// Something follows the top-level value.
    EndOfFileExpected,
    /// A `/*` comment is not closed.
    UnexpectedEndOfComment,
    /// A string is not closed before the end of the line or file.
    UnexpectedEndOfString,
    /// A number ends after `.` or `e`.
    UnexpectedEndOfNumber,
    /// A `\u` escape is not followed by four hex digits.
    InvalidUnicode,
    /// An unknown escape such as `\x`.
    InvalidEscapeCharacter,
    /// A control character inside a string.
    InvalidCharacter,
    /// Arrays and objects nest too deeply (rustode only).
    TooDeep,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSymbol => "Invalid symbol",
            Self::InvalidNumberFormat => "Invalid number format",
            Self::PropertyNameExpected => "Property name expected",
            Self::ValueExpected => "Value expected",
            Self::ColonExpected => "Colon expected",
            Self::CommaExpected => "Comma expected",
            Self::CloseBraceExpected => "Closing brace expected",
            Self::CloseBracketExpected => "Closing bracket expected",
            Self::EndOfFileExpected => "End of file expected",
            Self::UnexpectedEndOfComment => "Unexpected end of comment",
            Self::UnexpectedEndOfString => "Unexpected end of string",
            Self::UnexpectedEndOfNumber => "Unexpected end of number",
            Self::InvalidUnicode => "Invalid unicode sequence in string",
            Self::InvalidEscapeCharacter => "Invalid escape character in string",
            Self::InvalidCharacter => "Invalid characters in string",
            Self::TooDeep => "Nesting is too deep",
        })
    }
}

/// A syntax error and where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    /// What is wrong.
    pub error: JsonError,
    /// The token where it was found.
    pub span: Span,
}

/// The result of [`parse`]: the value that could be recovered and every error.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    /// The top-level value; `None` for an empty document or when nothing could be recovered.
    pub root: Option<Node>,
    /// Syntax errors in source order.
    pub errors: Vec<ParseError>,
}

/// Parses JSON with comments and trailing commas. Never fails: errors are collected and as much of
/// the document as possible is returned.
#[must_use]
pub fn parse(text: &str) -> Parsed {
    let mut parser = Parser {
        lexer: Lexer { text, pos: 0 },
        token: Token::eof(0),
        previous_end: 0,
        errors: Vec::new(),
        depth: 0,
    };
    parser.scan_next();
    if parser.token.kind == Tok::Eof {
        return Parsed {
            root: None,
            errors: parser.errors,
        };
    }
    let root = parser.parse_value();
    if root.is_none() {
        parser.error(JsonError::ValueExpected, &[], &[]);
    } else if parser.token.kind != Tok::Eof {
        parser.error(JsonError::EndOfFileExpected, &[], &[]);
    }
    Parsed {
        root,
        errors: parser.errors,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Comma,
    Colon,
    Null,
    True,
    False,
    String,
    Number,
    Trivia,
    Unknown,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Tok,
    span: Span,
    /// The decoded string, or the number's text.
    value: String,
}

impl Token {
    fn eof(at: usize) -> Self {
        Self {
            kind: Tok::Eof,
            span: Span { start: at, end: at },
            value: String::new(),
        }
    }
}

/// Whitespace as in `isWhitespace` (`json.ts:561`), the BOM included.
fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\u{b}' | '\u{c}' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200b}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// Line breaks as in `isLineBreak` (`json.ts:567`).
fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

struct Lexer<'a> {
    text: &'a str,
    pos: usize,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn peek_byte(&self, ahead: usize) -> Option<u8> {
        self.text.as_bytes().get(self.pos + ahead).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// The next token, trivia included, and the scan error it carries (`json.ts:365`).
    fn scan(&mut self) -> (Token, Option<JsonError>) {
        let start = self.pos;
        let mut error = None;
        let mut value = String::new();
        let Some(c) = self.bump() else {
            return (Token::eof(start), None);
        };
        let kind = match c {
            c if is_whitespace(c) || is_line_break(c) => {
                while self
                    .peek()
                    .is_some_and(|c| is_whitespace(c) || is_line_break(c))
                {
                    self.bump();
                }
                Tok::Trivia
            }
            '{' => Tok::OpenBrace,
            '}' => Tok::CloseBrace,
            '[' => Tok::OpenBracket,
            ']' => Tok::CloseBracket,
            ',' => Tok::Comma,
            ':' => Tok::Colon,
            '"' => {
                (value, error) = self.string();
                Tok::String
            }
            '/' if self.peek() == Some('/') => {
                while self.peek().is_some_and(|c| !is_line_break(c)) {
                    self.bump();
                }
                Tok::Trivia
            }
            '/' if self.peek() == Some('*') => {
                self.bump();
                if let Some(end) = self.text[self.pos..].find("*/") {
                    self.pos += end + 2;
                } else {
                    self.pos = self.text.len();
                    error = Some(JsonError::UnexpectedEndOfComment);
                }
                Tok::Trivia
            }
            '-' if !self.peek().is_some_and(|c| c.is_ascii_digit()) => Tok::Unknown,
            '-' | '0'..='9' => {
                let end;
                (error, end) = self.number(c);
                self.text[start..end].clone_into(&mut value);
                Tok::Number
            }
            _ => self.word(start),
        };
        let token = Token {
            kind,
            span: Span {
                start,
                end: self.pos,
            },
            value,
        };
        (token, error)
    }

    /// A keyword, or an unknown run of characters (`json.ts:500`).
    fn word(&mut self, start: usize) -> Tok {
        let is_word = |c: char| {
            !(is_whitespace(c)
                || is_line_break(c)
                || matches!(c, '{' | '}' | '[' | ']' | '"' | ':' | ',' | '/'))
        };
        if is_word(self.text[start..].chars().next().unwrap_or(' ')) {
            while self.peek().is_some_and(is_word) {
                self.bump();
            }
        }
        match &self.text[start..self.pos] {
            "true" => Tok::True,
            "false" => Tok::False,
            "null" => Tok::Null,
            _ => Tok::Unknown,
        }
    }

    /// Digits after the first one (`scanNumber`, `json.ts:244`). Returns the scan error and where
    /// the number's value ends: an exponent without digits is consumed but left out of the value.
    fn number(&mut self, first: char) -> (Option<JsonError>, usize) {
        let digits = |lexer: &mut Self| {
            let mut any = false;
            while lexer.peek_byte(0).is_some_and(|b| b.is_ascii_digit()) {
                lexer.pos += 1;
                any = true;
            }
            any
        };
        if first == '-' {
            // The scanner only gets here when a digit follows.
            let zero = self.peek_byte(0) == Some(b'0');
            self.pos += 1;
            if !zero {
                digits(self);
            }
        } else if first != '0' {
            digits(self);
        }
        if self.peek_byte(0) == Some(b'.') {
            self.pos += 1;
            if !digits(self) {
                return (Some(JsonError::UnexpectedEndOfNumber), self.pos);
            }
        }
        let end = self.pos;
        if matches!(self.peek_byte(0), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek_byte(0), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !digits(self) {
                return (Some(JsonError::UnexpectedEndOfNumber), end);
            }
        }
        (None, self.pos)
    }

    /// The rest of a string after its opening quote (`scanString`, `json.ts:285`).
    fn string(&mut self) -> (String, Option<JsonError>) {
        let mut out = String::new();
        let mut error = None;
        loop {
            let Some(c) = self.peek() else {
                return (out, Some(JsonError::UnexpectedEndOfString));
            };
            match c {
                '"' => {
                    self.bump();
                    return (out, error);
                }
                '\\' => {
                    self.bump();
                    let Some(escape) = self.bump() else {
                        return (out, Some(JsonError::UnexpectedEndOfString));
                    };
                    match escape {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => match self.unicode_escape() {
                            Some(c) => out.push(c),
                            None => error = Some(JsonError::InvalidUnicode),
                        },
                        _ => error = Some(JsonError::InvalidEscapeCharacter),
                    }
                }
                c if is_line_break(c) => return (out, Some(JsonError::UnexpectedEndOfString)),
                c if c < ' ' => {
                    error = Some(JsonError::InvalidCharacter);
                    out.push(c);
                    self.bump();
                }
                c => {
                    out.push(c);
                    self.bump();
                }
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.text.get(self.pos..self.pos + 4)?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.pos += 4;
        u32::from_str_radix(digits, 16).ok()
    }

    /// `\uXXXX`, joining a surrogate pair written as two escapes. A lone surrogate, which a
    /// JavaScript string can hold but a Rust one cannot, becomes U+FFFD.
    fn unicode_escape(&mut self) -> Option<char> {
        let unit = self.hex4()?;
        if (0xD800..0xDC00).contains(&unit) && self.text[self.pos..].starts_with("\\u") {
            let save = self.pos;
            self.pos += 2;
            match self.hex4() {
                Some(low @ 0xDC00..0xE000) => {
                    let code = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                    return Some(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                _ => self.pos = save,
            }
        }
        Some(char::from_u32(unit).unwrap_or('\u{fffd}'))
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    token: Token,
    previous_end: usize,
    errors: Vec<ParseError>,
    depth: usize,
}

impl Parser<'_> {
    /// Moves to the next significant token, reporting scan errors and unknown symbols on the way
    /// (`scanNext` in `visit`, `json.ts:1110`).
    fn scan_next(&mut self) {
        self.previous_end = self.token.span.end;
        loop {
            let (token, error) = self.lexer.scan();
            if let Some(error) = error {
                self.errors.push(ParseError {
                    error,
                    span: token.span,
                });
            }
            match token.kind {
                Tok::Trivia => {}
                Tok::Unknown => self.errors.push(ParseError {
                    error: JsonError::InvalidSymbol,
                    span: token.span,
                }),
                _ => {
                    self.token = token;
                    return;
                }
            }
        }
    }

    /// Reports an error at the current token, then skips past a token in `skip_after` or up to a
    /// token in `skip_until` (`handleError`, `json.ts:1156`).
    fn error(&mut self, error: JsonError, skip_after: &[Tok], skip_until: &[Tok]) {
        self.errors.push(ParseError {
            error,
            span: self.token.span,
        });
        if skip_after.is_empty() && skip_until.is_empty() {
            return;
        }
        while self.token.kind != Tok::Eof {
            if skip_after.contains(&self.token.kind) {
                self.scan_next();
                return;
            }
            if skip_until.contains(&self.token.kind) {
                return;
            }
            self.scan_next();
        }
    }

    /// `parseValue` (`json.ts:1298`).
    fn parse_value(&mut self) -> Option<Node> {
        let token = self.token.clone();
        let kind = match token.kind {
            Tok::OpenBrace | Tok::OpenBracket => {
                if self.depth >= MAX_DEPTH {
                    self.error(JsonError::TooDeep, &[], &[]);
                    while self.token.kind != Tok::Eof {
                        self.scan_next();
                    }
                    return None;
                }
                self.depth += 1;
                let kind = if token.kind == Tok::OpenBrace {
                    NodeKind::Object(self.parse_object())
                } else {
                    NodeKind::Array(self.parse_array())
                };
                self.depth -= 1;
                return Some(Node {
                    span: Span {
                        start: token.span.start,
                        end: self.previous_end,
                    },
                    kind,
                });
            }
            Tok::String => NodeKind::String(token.value),
            // `JSON.parse` rejects a trailing `.`, which Rust's parser accepts.
            Tok::Number => match token.value.parse::<f64>() {
                Ok(n) if n.is_finite() && !token.value.ends_with('.') => NodeKind::Number(n),
                _ => {
                    self.error(JsonError::InvalidNumberFormat, &[], &[]);
                    NodeKind::Number(0.0)
                }
            },
            Tok::Null => NodeKind::Null,
            Tok::True => NodeKind::Bool(true),
            Tok::False => NodeKind::Bool(false),
            _ => return None,
        };
        self.scan_next();
        Some(Node {
            span: token.span,
            kind,
        })
    }

    /// `parseObject` (`json.ts:1234`); the opening brace is the current token.
    fn parse_object(&mut self) -> Vec<Property> {
        self.scan_next();
        let mut properties = Vec::new();
        let mut needs_comma = false;
        while !matches!(self.token.kind, Tok::CloseBrace | Tok::Eof) {
            if self.token.kind == Tok::Comma {
                if !needs_comma {
                    self.error(JsonError::ValueExpected, &[], &[]);
                }
                self.scan_next();
                if self.token.kind == Tok::CloseBrace {
                    break;
                }
            } else if needs_comma {
                self.error(JsonError::CommaExpected, &[], &[]);
            }
            if !self.parse_property(&mut properties) {
                self.error(
                    JsonError::ValueExpected,
                    &[],
                    &[Tok::CloseBrace, Tok::Comma],
                );
            }
            needs_comma = true;
        }
        if self.token.kind == Tok::CloseBrace {
            self.scan_next();
        } else {
            self.error(JsonError::CloseBraceExpected, &[Tok::CloseBrace], &[]);
        }
        properties
    }

    /// `parseProperty` (`json.ts:1215`).
    fn parse_property(&mut self, properties: &mut Vec<Property>) -> bool {
        if self.token.kind != Tok::String {
            self.error(
                JsonError::PropertyNameExpected,
                &[],
                &[Tok::CloseBrace, Tok::Comma],
            );
            return false;
        }
        let key = std::mem::take(&mut self.token.value);
        let key_span = self.token.span;
        self.scan_next();
        let mut value = None;
        if self.token.kind == Tok::Colon {
            self.scan_next();
            value = self.parse_value();
            if value.is_none() {
                self.error(
                    JsonError::ValueExpected,
                    &[],
                    &[Tok::CloseBrace, Tok::Comma],
                );
            }
        } else {
            self.error(
                JsonError::ColonExpected,
                &[],
                &[Tok::CloseBrace, Tok::Comma],
            );
        }
        properties.push(Property {
            key,
            key_span,
            value,
        });
        true
    }

    /// `parseArray` (`json.ts:1266`); the opening bracket is the current token.
    fn parse_array(&mut self) -> Vec<Node> {
        self.scan_next();
        let mut items = Vec::new();
        let mut needs_comma = false;
        while !matches!(self.token.kind, Tok::CloseBracket | Tok::Eof) {
            if self.token.kind == Tok::Comma {
                if !needs_comma {
                    self.error(JsonError::ValueExpected, &[], &[]);
                }
                self.scan_next();
                if self.token.kind == Tok::CloseBracket {
                    break;
                }
            } else if needs_comma {
                self.error(JsonError::CommaExpected, &[], &[]);
            }
            match self.parse_value() {
                Some(node) => items.push(node),
                None => self.error(
                    JsonError::ValueExpected,
                    &[],
                    &[Tok::CloseBracket, Tok::Comma],
                ),
            }
            needs_comma = true;
        }
        if self.token.kind == Tok::CloseBracket {
            self.scan_next();
        } else {
            self.error(JsonError::CloseBracketExpected, &[Tok::CloseBracket], &[]);
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Value {
        let parsed = parse(text);
        assert!(parsed.errors.is_empty(), "{text}: {:?}", parsed.errors);
        parsed.root.expect("a value").to_value()
    }

    fn errors(text: &str) -> Vec<(JsonError, usize)> {
        parse(text)
            .errors
            .iter()
            .map(|e| (e.error, e.span.start))
            .collect()
    }

    #[test]
    fn plain_json_parses() {
        let v = value(r#"{"a": [1, -2.5e3, true, false, null, "x\n\u00e9\ud83d\ude00"], "b": {}}"#);
        let Value::Object(map) = v else {
            panic!("object expected")
        };
        assert_eq!(
            map["a"],
            Value::Array(vec![
                Value::Number(1.0),
                Value::Number(-2500.0),
                Value::Bool(true),
                Value::Bool(false),
                Value::Null,
                Value::String("x\né😀".into()),
            ])
        );
        assert_eq!(map["b"], Value::Object(BTreeMap::new()));
    }

    #[test]
    fn comments_and_trailing_commas_are_accepted() {
        assert_eq!(
            value("// head\n[ /* one */ 1, 2, // two\n ]"),
            Value::Array(vec![Value::Number(1.0), Value::Number(2.0)])
        );
        assert_eq!(value("{\"a\": 1,}"), value("{\"a\": 1}"));
    }

    #[test]
    fn empty_documents_are_not_errors() {
        for text in ["", "  \n", "// only a comment", "\u{feff}"] {
            let parsed = parse(text);
            assert_eq!(parsed.root, None);
            assert!(parsed.errors.is_empty(), "{text:?}");
        }
    }

    #[test]
    fn errors_are_located_and_parsing_recovers() {
        assert_eq!(errors("[1 2]"), vec![(JsonError::CommaExpected, 3)]);
        assert_eq!(errors("{\"a\" 1}"), vec![(JsonError::ColonExpected, 5)]);
        assert_eq!(errors("{\"a\": }"), vec![(JsonError::ValueExpected, 6)]);
        assert_eq!(errors("[1,,2]"), vec![(JsonError::ValueExpected, 3)]);
        assert_eq!(
            errors("{1: 2}"),
            vec![
                (JsonError::PropertyNameExpected, 1),
                (JsonError::ValueExpected, 5),
            ]
        );
        assert_eq!(errors("[1"), vec![(JsonError::CloseBracketExpected, 2)]);
        assert_eq!(errors("{"), vec![(JsonError::CloseBraceExpected, 1)]);
        assert_eq!(errors("1 2"), vec![(JsonError::EndOfFileExpected, 2)]);
        assert_eq!(errors("[x]"), vec![(JsonError::InvalidSymbol, 1)]);
        let parsed = parse("{\"a\": 1 \"b\": 2}");
        assert_eq!(parsed.errors.len(), 1);
        let Value::Object(map) = parsed.root.expect("root").to_value() else {
            panic!("object expected")
        };
        assert_eq!(map.len(), 2, "both members survive the missing comma");
    }

    #[test]
    fn string_and_number_errors() {
        assert_eq!(errors("\"abc"), vec![(JsonError::UnexpectedEndOfString, 0)]);
        assert_eq!(
            errors("\"a\nb\""),
            vec![
                (JsonError::UnexpectedEndOfString, 0),
                (JsonError::InvalidSymbol, 3),
                (JsonError::UnexpectedEndOfString, 4),
                (JsonError::EndOfFileExpected, 4),
            ]
        );
        assert_eq!(
            errors("\"\\x\""),
            vec![(JsonError::InvalidEscapeCharacter, 0)]
        );
        assert_eq!(errors("\"\\u12\""), vec![(JsonError::InvalidUnicode, 0)]);
        assert_eq!(errors("\"\u{1}\""), vec![(JsonError::InvalidCharacter, 0)]);
        assert_eq!(
            errors("1."),
            vec![
                (JsonError::UnexpectedEndOfNumber, 0),
                (JsonError::InvalidNumberFormat, 0),
            ]
        );
        assert_eq!(errors("1e"), vec![(JsonError::UnexpectedEndOfNumber, 0)]);
        assert_eq!(
            errors("/* open"),
            vec![(JsonError::UnexpectedEndOfComment, 0)]
        );
        assert_eq!(errors("-"), vec![(JsonError::InvalidSymbol, 0),]);
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let text = "[".repeat(10_000);
        let found = errors(&text);
        assert!(
            found.contains(&(JsonError::TooDeep, MAX_DEPTH)),
            "{found:?}"
        );
    }

    #[test]
    fn spans_cover_the_value() {
        let parsed = parse("{ \"k\": [1, 2] }");
        let root = parsed.root.expect("root");
        assert_eq!(root.span, Span { start: 0, end: 15 });
        let NodeKind::Object(props) = root.kind else {
            panic!("object expected")
        };
        assert_eq!(props[0].key_span, Span { start: 2, end: 5 });
        assert_eq!(
            props[0].value.as_ref().expect("value").span,
            Span { start: 7, end: 13 }
        );
    }
}
