//! `keybindings.json`: an array of `{ "key", "command", "when", "args" }` entries.

use crate::diagnostic::{Diagnostic, DiagnosticKind, LineIndex, Severity};
use crate::jsonc::{self, Node, NodeKind, Property, Value};
use crate::when::{When, parse_when};
use crate::{Location, syntax_diagnostics};

/// One key press: modifiers and a key name, like VS Code's `KeyCodeChord`.
#[allow(clippy::struct_excessive_bools, reason = "one flag per modifier key")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    /// `ctrl`.
    pub ctrl: bool,
    /// `shift`.
    pub shift: bool,
    /// `alt`.
    pub alt: bool,
    /// `meta`, also written `cmd` or `win`.
    pub meta: bool,
    /// The key, lowercased as VS Code reads it: `k`, `f5`, `enter`, or a scan code such as
    /// `[keya]`.
    pub key: String,
}

/// One entry of `keybindings.json`, read as `KeybindingIO.readUserKeybindingItem` reads it
/// (`src/vs/workbench/services/keybinding/common/keybindingIO.ts:51`).
#[derive(Debug, Clone, PartialEq)]
pub struct Keybinding {
    /// The chords to press in sequence; `None` when the entry has no usable `key`, which is
    /// meaningful for a removal (it removes every default keybinding of the command).
    pub key: Option<Vec<Chord>>,
    /// The `key` text as written.
    pub key_source: Option<String>,
    /// The command; a leading `-` removes a default keybinding instead of adding one.
    pub command: Option<String>,
    /// The command's arguments.
    pub args: Option<Value>,
    /// The condition; `None` means always.
    pub when: Option<When>,
    /// `systemWide`.
    pub system_wide: bool,
    /// Where the entry starts in the file.
    pub position: Location,
}

impl Keybinding {
    /// Whether the entry removes a default keybinding (`"command": "-…"`).
    #[must_use]
    pub fn is_removal(&self) -> bool {
        self.command.as_deref().is_some_and(|c| c.starts_with('-'))
    }

    /// Whether two entries bind the same thing, ignoring where they are in the file.
    #[must_use]
    pub fn same_binding(&self, other: &Self) -> bool {
        self.key == other.key
            && self.command == other.command
            && self.args == other.args
            && self.when == other.when
            && self.system_wide == other.system_wide
    }
}

/// The parsed file: the usable entries and every problem found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Keybindings {
    /// Entries in file order.
    pub bindings: Vec<Keybinding>,
    /// Problems in file order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses a key such as `ctrl+shift+k` or the chord sequence `ctrl+k ctrl+c`
/// (`KeybindingParser.parseKeybinding`, `src/vs/base/common/keybindingParser.ts:88`). Modifiers
/// are joined with `+` or `-`, in any order. Returns `None` for an empty key or a chord without
/// a key name.
#[must_use]
pub fn parse_key(input: &str) -> Option<Vec<Chord>> {
    let mut chords = Vec::new();
    let mut rest = input.to_lowercase();
    while !rest.trim().is_empty() {
        let (chord, remains) = parse_chord(rest.trim());
        if chord.key.is_empty() {
            return None;
        }
        chords.push(chord);
        rest = remains;
    }
    (!chords.is_empty()).then_some(chords)
}

/// Modifier names; `meta`, `win` and `cmd` are the same modifier.
const MODIFIERS: [&str; 6] = ["ctrl", "shift", "alt", "meta", "win", "cmd"];

/// `_readModifiers` (`keybindingParser.ts:11`).
fn parse_chord(mut input: &str) -> (Chord, String) {
    let mut chord = Chord {
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
        key: String::new(),
    };
    while let Some((which, after)) = MODIFIERS.iter().enumerate().find_map(|(i, name)| {
        let after = input.strip_prefix(name)?.strip_prefix(['+', '-'])?;
        Some((i, after))
    }) {
        match which {
            0 => chord.ctrl = true,
            1 => chord.shift = true,
            2 => chord.alt = true,
            _ => chord.meta = true,
        }
        input = after;
    }
    let (key, remains) = input.split_once(' ').unwrap_or((input, ""));
    key.clone_into(&mut chord.key);
    (chord, remains.to_owned())
}

/// Parses `keybindings.json`. Never fails: problems become diagnostics.
///
/// Like VS Code (`src/vs/workbench/services/keybinding/browser/keybindingService.ts:821`), a file
/// whose top level is not an array yields no keybindings, and entries that are not objects are
/// skipped. Unlike VS Code, an entry whose `when` does not parse is dropped and reported: VS Code
/// keeps it with no condition (`contextkey.ts:629` returns `undefined`), which makes the key fire
/// everywhere.
#[must_use]
pub fn parse_keybindings(text: &str) -> Keybindings {
    let parsed = jsonc::parse(text);
    let index = LineIndex::new(text);
    let mut out = Keybindings {
        bindings: Vec::new(),
        diagnostics: syntax_diagnostics(&index, &parsed.errors),
    };
    let Some(root) = parsed.root else {
        return out;
    };
    let NodeKind::Array(entries) = &root.kind else {
        out.diagnostics.push(index.diagnostic(
            DiagnosticKind::InvalidKeybinding,
            Severity::Error,
            root.span.start,
            format!(
                "keybindings.json must be an array of keybindings, not {}",
                root.to_value().type_name()
            ),
        ));
        return out;
    };
    for entry in entries {
        if let Some(binding) = read_entry(text, &index, entry, &mut out.diagnostics) {
            out.bindings.push(binding);
        }
    }
    out.diagnostics.sort_by_key(|d| d.position);
    out
}

fn read_entry(
    text: &str,
    index: &LineIndex<'_>,
    entry: &Node,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Keybinding> {
    let mut reader = Entry {
        index,
        diagnostics,
        properties: &[],
    };
    let NodeKind::Object(properties) = &entry.kind else {
        reader.warn(
            entry.span.start,
            format!(
                "A keybinding must be an object, not {}; it is ignored",
                entry.to_value().type_name()
            ),
        );
        return None;
    };
    reader.properties = properties;
    let mut binding = Keybinding {
        key: None,
        key_source: reader.string("key"),
        command: reader.string("command"),
        args: reader.field("args").map(Node::to_value),
        when: None,
        system_wide: matches!(
            reader.field("systemWide").map(|n| &n.kind),
            Some(NodeKind::Bool(true))
        ),
        position: index.position(entry.span.start),
    };
    reader.check_strings();
    if reader.field("command").is_none() {
        reader.warn(
            entry.span.start,
            "The keybinding has no \"command\"".to_owned(),
        );
    }
    match (&binding.key_source, reader.field("key")) {
        (Some(source), Some(node)) => {
            binding.key = parse_key(source);
            if binding.key.is_none() {
                reader.warn(node.span.start, format!("Invalid key '{source}'"));
            }
        }
        (None, None) if !binding.is_removal() => {
            reader.warn(entry.span.start, "The keybinding has no \"key\"".to_owned());
        }
        _ => {}
    }
    binding.when = reader.when(text).ok()?;
    Some(binding)
}

/// Reads the properties of one keybinding object.
struct Entry<'a, 'b> {
    index: &'a LineIndex<'a>,
    diagnostics: &'b mut Vec<Diagnostic>,
    properties: &'a [Property],
}

impl<'a> Entry<'a, '_> {
    fn warn(&mut self, offset: usize, message: String) {
        self.diagnostics.push(self.index.diagnostic(
            DiagnosticKind::InvalidKeybinding,
            Severity::Warning,
            offset,
            message,
        ));
    }

    /// The value of a property; the last one wins when it is repeated.
    fn field(&self, name: &str) -> Option<&'a Node> {
        self.properties
            .iter()
            .rev()
            .find(|p| p.key == name)
            .and_then(|p| p.value.as_ref())
    }

    fn string(&self, name: &str) -> Option<String> {
        match self.field(name).map(|n| &n.kind) {
            Some(NodeKind::String(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Reports `key`, `command` and `when` values that are not strings; VS Code ignores them.
    fn check_strings(&mut self) {
        for name in ["key", "command", "when"] {
            if let Some(node) = self.field(name)
                && !matches!(node.kind, NodeKind::String(_))
            {
                let message = format!(
                    "\"{name}\" must be a string, not {}; it is ignored",
                    node.to_value().type_name()
                );
                self.warn(node.span.start, message);
            }
        }
    }

    /// The parsed `when`, `Ok(None)` when there is none, or `Err` (already reported) when it
    /// does not parse, which drops the keybinding.
    fn when(&mut self, text: &str) -> Result<Option<When>, Dropped> {
        let Some(node) = self.field("when") else {
            return Ok(None);
        };
        let NodeKind::String(clause) = &node.kind else {
            return Ok(None);
        };
        match parse_when(clause) {
            Ok(when) => Ok(Some(when)),
            Err(error) => {
                self.diagnostics.push(self.index.diagnostic(
                    DiagnosticKind::InvalidWhen,
                    Severity::Error,
                    offset_in_string(text, node, error.offset),
                    format!(
                        "Invalid \"when\": {}; the keybinding is ignored",
                        error.message
                    ),
                ));
                Err(Dropped)
            }
        }
    }
}

/// A keybinding was dropped after reporting why.
struct Dropped;

/// The source offset of byte `offset` of a string's decoded value. When the string has escapes,
/// decoded and source offsets differ, so the string's start is used instead (VS Code has the same
/// limitation, `scanner.ts:81`).
fn offset_in_string(text: &str, node: &Node, offset: usize) -> usize {
    let NodeKind::String(value) = &node.kind else {
        return node.span.start;
    };
    let raw = text.get(node.span.start + 1..node.span.end.saturating_sub(1));
    if raw == Some(value.as_str()) {
        node.span.start + 1 + offset
    } else {
        node.span.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(mods: &str, key: &str) -> Chord {
        Chord {
            ctrl: mods.contains('c'),
            shift: mods.contains('s'),
            alt: mods.contains('a'),
            meta: mods.contains('m'),
            key: key.to_owned(),
        }
    }

    #[test]
    fn keys_and_chords() {
        assert_eq!(parse_key("ctrl+shift+k"), Some(vec![chord("cs", "k")]));
        assert_eq!(parse_key("Shift-Ctrl-K"), Some(vec![chord("cs", "k")]));
        assert_eq!(parse_key("cmd+alt+f1"), Some(vec![chord("am", "f1")]));
        assert_eq!(parse_key("win+[KeyA]"), Some(vec![chord("m", "[keya]")]));
        assert_eq!(
            parse_key("ctrl+k  ctrl+c"),
            Some(vec![chord("c", "k"), chord("c", "c")])
        );
        assert_eq!(parse_key("ctrl++"), Some(vec![chord("c", "+")]));
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("ctrl+"), None);
    }

    #[test]
    fn entries_are_read_like_vs_code() {
        let parsed = parse_keybindings(
            r#"[
  { "key": "f5", "command": "run", "args": { "x": [1] }, "systemWide": true },
  { "command": "-editor.action.save" },
  42,
  { "key": 7, "command": "x" },
  { "key": "f6" },
]"#,
        );
        assert_eq!(parsed.bindings.len(), 4);
        assert!(parsed.bindings[0].system_wide);
        assert!(parsed.bindings[1].is_removal());
        assert_eq!(parsed.bindings[1].key, None);
        assert_eq!(parsed.bindings[2].key, None);
        let found: Vec<_> = parsed
            .diagnostics
            .iter()
            .map(|d| (d.kind, d.position.line, d.position.column))
            .collect();
        assert_eq!(
            found,
            vec![
                (DiagnosticKind::InvalidKeybinding, 4, 3),
                (DiagnosticKind::InvalidKeybinding, 5, 12),
                (DiagnosticKind::InvalidKeybinding, 6, 3),
            ]
        );
    }

    #[test]
    fn when_errors_point_into_the_string_unless_it_has_escapes() {
        let plain = parse_keybindings(r#"[{"key": "a", "command": "c", "when": "x &&"}]"#);
        assert_eq!(plain.diagnostics[0].position.column, 44);
        let escaped = parse_keybindings(r#"[{"key": "a", "command": "c", "when": "\u0078 &&"}]"#);
        assert_eq!(escaped.diagnostics[0].position.column, 39);
        assert!(plain.bindings.is_empty() && escaped.bindings.is_empty());
    }

    #[test]
    fn not_an_array() {
        let parsed = parse_keybindings("{}");
        assert_eq!(parsed.bindings.len(), 0);
        assert_eq!(parsed.diagnostics[0].severity, Severity::Error);
    }
}
