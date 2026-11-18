//! What changed between the text a server last saw and the document's text
//! now, as one replaced range: `didChange` sends it (or the whole text, for
//! servers that sync fully), and markers move with it.
//!
//! The change is found by comparing the two texts, which costs a pass over
//! each; the editor core does not report its edits yet.

use super::encoding::PositionEncoding;
use serde_json::{Value, json};

/// One replaced range, in chars and in bytes of the old text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    /// First changed char.
    pub start: usize,
    /// End of the replaced chars in the old text.
    pub old_end: usize,
    /// The text that replaced them.
    pub inserted: String,
    start_byte: usize,
    old_end_byte: usize,
}

impl Change {
    /// The replaced range as an LSP range in `old`, the text the change
    /// applies to.
    pub(crate) fn lsp_range(&self, old: &str, encoding: PositionEncoding) -> Value {
        json!({
            "start": position(old, self.start_byte, encoding),
            "end": position(old, self.old_end_byte, encoding),
        })
    }
}

/// The change from `old` to `new`; `None` when they are equal. It never
/// starts or ends between the CR and the LF of a CRLF, which LSP counts as
/// one line break.
pub(crate) fn diff(old: &str, new: &str) -> Option<Change> {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !(old.is_char_boundary(prefix) && new.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    if prefix > 0 && a[prefix - 1] == b'\r' {
        prefix -= 1;
    }
    let room = a.len().min(b.len()) - prefix;
    let mut suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(room)
        .take_while(|(x, y)| x == y)
        .count();
    while !(old.is_char_boundary(a.len() - suffix) && new.is_char_boundary(b.len() - suffix)) {
        suffix -= 1;
    }
    if suffix > 0 && a[a.len() - suffix] == b'\n' {
        suffix -= 1;
    }
    let (old_end_byte, new_end_byte) = (a.len() - suffix, b.len() - suffix);
    if prefix == old_end_byte && prefix == new_end_byte {
        return None;
    }
    let start = old[..prefix].chars().count();
    Some(Change {
        start,
        old_end: start + old[prefix..old_end_byte].chars().count(),
        inserted: new[prefix..new_end_byte].to_owned(),
        start_byte: prefix,
        old_end_byte,
    })
}

/// The LSP position of byte `at` in `text`: lines end at CRLF, LF or CR.
fn position(text: &str, at: usize, encoding: PositionEncoding) -> Value {
    let before = &text.as_bytes()[..at];
    let mut line = 0;
    let mut line_start = 0;
    for (i, byte) in before.iter().enumerate() {
        let ends = match byte {
            b'\n' => true,
            b'\r' => text.as_bytes().get(i + 1) != Some(&b'\n'),
            _ => false,
        };
        if ends {
            line += 1;
            line_start = i + 1;
        }
    }
    let content = &text[line_start..at];
    let character = encoding.units_before(content, usize::MAX);
    json!({"line": line, "character": character})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(old: &str, new: &str) -> (usize, usize, String) {
        let change = diff(old, new).expect("a change");
        (change.start, change.old_end, change.inserted)
    }

    #[test]
    fn the_change_is_the_part_between_the_common_ends() {
        assert_eq!(diff("same", "same"), None);
        assert_eq!(change("abc", "aXc"), (1, 2, "X".into()));
        assert_eq!(change("aaa", "aaaa"), (3, 3, "a".into()));
        assert_eq!(change("é", "è"), (0, 1, "è".into()), "never inside a char");
        assert_eq!(
            change("a\r\nb", "a\nb"),
            (1, 3, "\n".into()),
            "the CRLF goes whole"
        );
        assert_eq!(change("a\nb", "a\r\nb"), (1, 2, "\r\n".into()));
        assert_eq!(change("x\r\n", "x\r\ny\r\n"), (3, 3, "y\r\n".into()));
    }

    #[test]
    fn positions_count_lines_and_units() {
        let text = "a\r\nb\rc\né😀x";
        let range = diff(text, "a\r\nb\rc\né😀")
            .expect("change")
            .lsp_range(text, PositionEncoding::Utf16);
        assert_eq!(range["start"], json!({"line": 3, "character": 3}));
        let utf8 = diff(text, "a\r\nb\rc\né😀")
            .expect("change")
            .lsp_range(text, PositionEncoding::Utf8);
        assert_eq!(utf8["end"], json!({"line": 3, "character": 7}));
    }
}
