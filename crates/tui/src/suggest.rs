//! The hover and suggest popups: keys that open and drive them, and how
//! they are drawn next to the cursor. What they hold, how the list is
//! filtered and ordered and what accepting does are the workspace's
//! (`rustode_workspace`'s `completion_*` and `hover`); this module only maps
//! keys and draws.
//!
//! Keys, as in VS Code (`src/vs/editor/contrib/suggest/browser/suggestController.ts:797-970`,
//! `src/vs/editor/contrib/hover/browser/hoverActions.ts:28-70`): Ctrl+Space
//! opens the list, Enter or Tab accepts, Escape closes, Up and Down move
//! (wrapping), Page Up and Page Down move by a page; Ctrl+K Ctrl+I shows the
//! hover at the cursor. A terminal sends Ctrl+I as Tab, so the second key of
//! the chord is either; and Ctrl+Space as Ctrl+Space or NUL. Any other key
//! hides the hover. The list shows eight rows at most, under the word being
//! completed, or above it when there is no room below; the hover goes above
//! the word, or under it when there is no room.
//!
//! Not done: the hover on the mouse pointer (terminals report motion only
//! in a mode this editor does not ask for), and the detail pane beside the
//! list.

use crate::app::{App, Focus};
use crate::view::{display_width, highlighted, to_u16, truncate};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};
use rustode_workspace::{CompletionMove, CompletionRow};
use unicode_width::UnicodeWidthStr;

/// Rows the list shows at most.
pub(crate) const LIST_ROWS: usize = 8;
/// The widest the list or the hover gets, in columns.
const MAX_WIDTH: usize = 60;
/// Lines of hover text shown.
const HOVER_LINES: usize = 10;

/// What the popups remember between keys.
#[derive(Debug, Default)]
pub(crate) struct State {
    /// Ctrl+K was pressed: the next key completes the chord.
    chord: bool,
}

impl App {
    /// Handles the keys of the popups; returns whether `key` was theirs.
    pub(crate) fn suggest_key(&mut self, key: KeyEvent) -> bool {
        let modifiers = key.modifiers;
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let plain = !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        if std::mem::take(&mut self.suggest.chord) {
            let second = (ctrl && key.code == KeyCode::Char('i')) || key.code == KeyCode::Tab;
            if second {
                self.run_editor(
                    "editor.action.showHover",
                    "Nothing to show a hover for here",
                );
                return true;
            }
        }
        if ctrl && key.code == KeyCode::Char('k') {
            self.workspace.hide_hover();
            self.suggest.chord = true;
            self.message = Some("(Ctrl+K) was pressed. Waiting for second key of chord...".into());
            return true;
        }
        let space = matches!(key.code, KeyCode::Null | KeyCode::Char('@' | ' '));
        if space && (ctrl || key.code == KeyCode::Null) {
            self.run_editor("editor.action.triggerSuggest", "No suggestions here");
            return true;
        }
        if self.workspace.completion_visible() && plain {
            let movement = match key.code {
                KeyCode::Up => Some(CompletionMove::Previous),
                KeyCode::Down => Some(CompletionMove::Next),
                KeyCode::PageUp => Some(CompletionMove::PageUp(LIST_ROWS)),
                KeyCode::PageDown => Some(CompletionMove::PageDown(LIST_ROWS)),
                _ => None,
            };
            if let Some(movement) = movement {
                self.workspace.completion_move(movement);
                return true;
            }
            match key.code {
                KeyCode::Enter | KeyCode::Tab => {
                    match self.workspace.completion_accept() {
                        Ok(_) => {}
                        Err(err) => self.message = Some(err.to_string()),
                    }
                    return true;
                }
                KeyCode::Esc => {
                    self.workspace.dismiss_completion();
                    return true;
                }
                _ => {}
            }
        }
        self.workspace.hide_hover();
        false
    }

    /// Runs an editor command of the registry; `none` is the message when
    /// the language server cannot answer.
    fn run_editor(&mut self, id: &str, none: &str) {
        self.message = None;
        match self.workspace.execute(id) {
            Ok(true) => {}
            Ok(false) => self.message = Some(none.to_owned()),
            Err(err) => self.message = Some(err.to_string()),
        }
    }
}

/// Draws the list, or else the hover, next to the cursor.
pub(crate) fn draw(app: &App, frame: &mut Frame) {
    let Some(hit) = app.hits.editor else {
        return;
    };
    if app.focus != Focus::Editor || app.quick.is_some() {
        return;
    }
    if let Some(list) = app.workspace.completion_list(LIST_ROWS) {
        if list.document != hit.id {
            return;
        }
        let Some((x, y)) = place(app, list.start) else {
            return;
        };
        let rows: Vec<Line<'static>> = list
            .rows
            .iter()
            .enumerate()
            .map(|(row, item)| list_line(app, item, list.first + row == list.selected))
            .collect();
        let width = rows.iter().map(Line::width).max().unwrap_or(0) + 1;
        let area = Rect {
            x,
            y,
            width: to_u16(width.min(MAX_WIDTH)),
            height: to_u16(rows.len()),
        };
        draw_popup(app, frame, area, &rows, false);
        return;
    }
    let Some(hover) = app.workspace.hover() else {
        return;
    };
    let cursor_at = app
        .workspace
        .document(hit.id)
        .map(rustode_workspace::Document::cursor);
    if hover.document != hit.id || cursor_at != Some(hover.offset) {
        return;
    }
    let Some((x, y)) = place(app, hover.anchor()) else {
        return;
    };
    let wrapped = wrap(&hover.text, MAX_WIDTH - 4);
    let width = wrapped.iter().map(|l| l.width()).max().unwrap_or(0) + 2;
    let rows: Vec<Line<'static>> = wrapped
        .into_iter()
        .take(HOVER_LINES)
        .map(|text| Line::from(format!(" {text}")))
        .collect();
    let area = Rect {
        x,
        y,
        width: to_u16(width + 2),
        height: to_u16(rows.len() + 2),
    };
    draw_popup(app, frame, area, &rows, true);
}

/// Where `offset` is on screen, as the cell of its char: `None` when it is
/// out of view.
fn place(app: &App, offset: usize) -> Option<(u16, u16)> {
    let hit = app.hits.editor?;
    let view = app.views.get(&hit.id)?;
    let document = app.workspace.document(hit.id)?;
    let position = document.position_of(offset);
    let height = usize::from(hit.area.height);
    if position.line < view.top || position.line >= view.top + height {
        return None;
    }
    let column = display_width(
        document.line(position.line).chars().take(position.column),
        hit.tab_size,
    );
    let column = column.checked_sub(view.left)?;
    Some((
        hit.area.x + hit.gutter + to_u16(column),
        hit.area.y + to_u16(position.line - view.top),
    ))
}

/// Puts `area` (whose `y` is the anchor's row) under the anchor, or above
/// it when it would not fit, and inside the editor; then draws the rows,
/// boxed when asked.
fn draw_popup(app: &App, frame: &mut Frame, area: Rect, rows: &[Line<'static>], boxed: bool) {
    let Some(hit) = app.hits.editor else {
        return;
    };
    let bounds = frame.area();
    let width = area.width.min(bounds.width);
    let height = area.height.min(bounds.height);
    let x = area.x.min(bounds.right().saturating_sub(width));
    // The list goes under the word, the hover above it, as in VS Code;
    // either goes to the other side when there is no room.
    let below = area.y + 1;
    let fits_below = below + height <= hit.area.bottom();
    let fits_above = area.y >= hit.area.y + height;
    let y = if (boxed && fits_above) || !fits_below {
        area.y.saturating_sub(height)
    } else {
        below
    };
    let outer = Rect {
        x,
        y,
        width,
        height,
    };
    let theme = app.theme;
    frame.render_widget(Clear, outer);
    frame.buffer_mut().set_style(outer, theme.quick);
    let inner = if boxed {
        let block = Block::bordered().border_style(theme.quick_border);
        let inner = block.inner(outer);
        frame.render_widget(block, outer);
        inner
    } else {
        outer
    };
    for (row, line) in rows.iter().enumerate().take(usize::from(inner.height)) {
        frame
            .buffer_mut()
            .set_line(inner.x, inner.y + to_u16(row), line, inner.width);
    }
}

/// One row of the list: the kind's letter, the label with the matched chars
/// marked, and the detail dimmed.
fn list_line(app: &App, item: &CompletionRow, selected: bool) -> Line<'static> {
    let theme = &app.theme;
    let mut spans = vec![Span::styled(
        format!(" {} ", item.kind.letter()),
        theme.description,
    )];
    let shifted: Vec<usize> = item.matched.clone();
    spans.extend(highlighted(&item.label, &shifted, theme.highlight));
    if let Some(detail) = &item.detail {
        let room = MAX_WIDTH.saturating_sub(item.label.width() + 6);
        spans.push(Span::styled(
            format!("  {}", truncate(detail, room)),
            theme.description,
        ));
    }
    let mut line = Line::from(spans);
    if selected {
        line = line.patch_style(theme.quick_selected);
    }
    line
}

/// `text` broken into lines of at most `width` columns at spaces; its own
/// line breaks stay, and a word wider than a line is cut.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let word = truncate(word, width);
            if !line.is_empty() && line.width() + 1 + word.width() > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&word);
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_wraps_at_spaces_and_keeps_its_line_breaks() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("a\n\nb", 10), ["a", "", "b"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd"], "a long word is cut");
    }
}
