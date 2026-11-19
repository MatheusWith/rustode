//! Hover: what a language server says about the text under a position.
//!
//! Studied in VS Code and reimplemented, see `ATTRIBUTION.md`: the hover
//! shows after `editor.hover.delay`, 300 ms by default
//! (`src/vs/editor/common/config/editorOptions.ts:2405`), which a frontend
//! waits before asking; `Ctrl+K Ctrl+I` (Show or Focus Hover) shows it at the
//! cursor at once
//! (`src/vs/editor/contrib/hover/browser/hoverActions.ts:28-70`); and it goes
//! away when the text changes. The server's markdown is shown as plain
//! text: code fences and code spans lose their marks, bold, headings and
//! links keep their words.
//!
//! One hover at a time: asking for another position cancels the request in
//! flight. An answer whose document changed meanwhile is dropped.

use super::requests::{Kind, Request, lsp_position};
use crate::{DocumentId, Error, Event, Range, Workspace};
use serde_json::Value;

/// What a server said about a position, as plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    /// The document.
    pub document: DocumentId,
    /// The char offset that was asked about.
    pub offset: usize,
    /// The text the hover is about, if the server said; else the word-sized
    /// neighbourhood is the frontend's to pick.
    pub range: Option<Range>,
    /// The content as plain text, paragraphs separated by a blank line.
    pub text: String,
}

impl Hover {
    /// Where a frontend anchors the popup: the start of the range, else the
    /// offset asked about.
    #[must_use]
    pub fn anchor(&self) -> usize {
        self.range.map_or(self.offset, |range| range.start)
    }
}

/// A hover asked for, and its answer once it came.
#[derive(Debug)]
pub(crate) struct State {
    document: DocumentId,
    version: u64,
    offset: usize,
    pub request: Option<Request>,
    result: Option<Hover>,
}

impl Workspace {
    /// Asks the document's language server what is at char `offset`. The
    /// answer arrives with [`poll_language_servers`](Self::poll_language_servers)
    /// as [`Event::HoverChanged`]; read it with [`hover`](Self::hover).
    /// Asking again for the same place while the text is the same does
    /// nothing; asking for another place cancels the earlier request.
    /// Returns whether a hover is, or will be, there to read: `false` when
    /// the document has no running server that offers hovers.
    ///
    /// # Errors
    /// When there is no such document.
    pub fn request_hover(&mut self, id: DocumentId, offset: usize) -> Result<bool, Error> {
        let doc = self.document(id).ok_or(Error::UnknownDocument(id))?;
        let (version, offset) = (doc.version(), offset.min(doc.len_chars()));
        if self
            .lsp
            .hover
            .as_ref()
            .is_some_and(|h| (h.document, h.version, h.offset) == (id, version, offset))
        {
            return Ok(true);
        }
        self.hide_hover();
        let Some((server, uri, encoding)) = self.lsp_target(id, |f| f.hover) else {
            return Ok(false);
        };
        let Some(doc) = self.document(id) else {
            return Ok(false);
        };
        let params = serde_json::json!({
            "textDocument": {"uri": uri},
            "position": lsp_position(doc, offset, encoding),
        });
        let Some(request) = self.lsp_request(server, Kind::Hover, "textDocument/hover", &params)
        else {
            return Ok(false);
        };
        self.lsp.hover = Some(State {
            document: id,
            version,
            offset,
            request: Some(request),
            result: None,
        });
        Ok(true)
    }

    /// The hover to show: the last answer for a position, while its
    /// document has not changed.
    #[must_use]
    pub fn hover(&self) -> Option<&Hover> {
        let state = self.lsp.hover.as_ref()?;
        let doc = self.document(state.document)?;
        if doc.version() != state.version {
            return None;
        }
        state.result.as_ref()
    }

    /// Whether a hover request is in flight.
    #[must_use]
    pub fn hover_pending(&self) -> bool {
        self.lsp.hover.as_ref().is_some_and(|h| h.request.is_some())
    }

    /// Hides the hover and cancels a request in flight.
    pub fn hide_hover(&mut self) {
        let Some(state) = self.lsp.hover.take() else {
            return;
        };
        if let Some(request) = state.request {
            self.lsp_cancel(request);
        }
        if state.result.is_some() {
            self.events.push(Event::HoverChanged);
        }
    }

    pub(super) fn hide_hover_of(&mut self, id: DocumentId) {
        if self.lsp.hover.as_ref().is_some_and(|h| h.document == id) {
            self.hide_hover();
        }
    }

    pub(super) fn hover_answered(&mut self, request: Request, result: &Value) {
        let Some(state) = self
            .lsp
            .hover
            .as_ref()
            .filter(|h| h.request == Some(request))
        else {
            return;
        };
        let (id, version, offset) = (state.document, state.version, state.offset);
        let current = self
            .document(id)
            .is_some_and(|doc| doc.version() == version);
        let encoding = self.lsp.server(request.0).map(|s| s.capabilities.encoding);
        let hover = (current)
            .then(|| encoding.and_then(|e| self.parse_hover(id, offset, result, e)))
            .flatten();
        if hover.is_none() {
            self.lsp.hover = None;
            return;
        }
        if let Some(state) = self.lsp.hover.as_mut() {
            state.request = None;
            state.result = hover;
        }
        self.events.push(Event::HoverChanged);
    }

    fn parse_hover(
        &self,
        id: DocumentId,
        offset: usize,
        result: &Value,
        encoding: super::encoding::PositionEncoding,
    ) -> Option<Hover> {
        let doc = self.document(id)?;
        let text = contents(&result["contents"]);
        if text.is_empty() {
            return None;
        }
        let range = result.get("range").filter(|r| r.is_object()).map(|range| {
            let start = super::offset(doc, &range["start"], encoding);
            let end = super::offset(doc, &range["end"], encoding).max(start);
            Range::new(start, end)
        });
        Some(Hover {
            document: id,
            offset,
            range,
            text,
        })
    }
}

/// The text of hover `contents`: a marked string, a markup content, or a
/// list of marked strings, as plain text.
pub(super) fn contents(value: &Value) -> String {
    let blocks: Vec<String> = match value {
        Value::Array(list) => list.iter().map(contents).collect(),
        Value::String(text) => vec![plain(text)],
        Value::Object(object) => {
            let text = object.get("value").and_then(Value::as_str).unwrap_or("");
            let verbatim = object.contains_key("language")
                || object.get("kind").and_then(Value::as_str) == Some("plaintext");
            if verbatim {
                vec![text.trim().to_owned()]
            } else {
                vec![plain(text)]
            }
        }
        _ => Vec::new(),
    };
    blocks
        .into_iter()
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Markdown as plain text: fence lines go (their content stays), headings,
/// block quotes, bold marks, code spans' backticks and the targets of links
/// are dropped; runs of blank lines become one.
pub(super) fn plain(markdown: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut fenced = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        let text = if fenced {
            line.trim_end().to_owned()
        } else {
            inline(
                trimmed
                    .trim_start_matches('#')
                    .trim_start_matches('>')
                    .trim(),
            )
        };
        if text.is_empty() && out.last().is_none_or(String::is_empty) {
            continue;
        }
        out.push(text);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.join("\n")
}

/// Inline markdown without its marks.
fn inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '`' => {}
            '*' if chars.get(i + 1) == Some(&'*') => i += 1,
            '[' => {
                // `[text](target)` keeps `text`.
                let close = chars[i..].iter().position(|&c| c == ']').map(|n| i + n);
                if let Some(close) = close.filter(|&c| chars.get(c + 1) == Some(&'(')) {
                    let end = chars[close..]
                        .iter()
                        .position(|&c| c == ')')
                        .map(|n| close + n);
                    if let Some(end) = end {
                        out.extend(&chars[i + 1..close]);
                        i = end;
                    } else {
                        out.push('[');
                    }
                } else {
                    out.push('[');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn markdown_becomes_plain_text() {
        let text = "# Title\n\n**bold** and `code` and [link](http://x.y)\n\n```rust\nlet x = 1;\n```\n\n\n> quote";
        assert_eq!(
            plain(text),
            "Title\n\nbold and code and link\n\nlet x = 1;\n\nquote"
        );
        assert_eq!(plain("snake_case *star*"), "snake_case *star*");
        assert_eq!(plain("[open"), "[open");
    }

    #[test]
    fn contents_come_in_four_shapes() {
        assert_eq!(contents(&json!("a `b`")), "a b");
        assert_eq!(
            contents(&json!({"language": "rust", "value": "fn f()"})),
            "fn f()"
        );
        assert_eq!(
            contents(&json!({"kind": "plaintext", "value": "**kept**"})),
            "**kept**"
        );
        assert_eq!(
            contents(&json!([{"kind": "markdown", "value": "**a**"}, "", "b"])),
            "a\n\nb"
        );
        assert_eq!(contents(&json!(null)), "");
    }
}
