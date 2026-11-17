//! Drawing: width bands, sidebar, tabs, editor and status bar.
//!
//! Only the visible lines are read from the workspace. Tabs advance to the
//! next tab stop of `editor.tabSize`. Invisible characters are drawn as VS
//! Code draws them (`src/vs/editor/common/viewLayout/viewLineRenderer.ts:728-758`,
//! `:1199-1209`): C0 controls as control pictures (U+2400 + code), DEL as ␡,
//! and bidirectional formatting characters as `[U+XXXX]`. Unicode line and
//! paragraph separators and NEL, which do not end lines, are shown the same
//! way so they cannot hide in a terminal.

use crate::app::{App, Focus, Viewport};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use rustode_workspace::{Document, DocumentId, Eol, Position, Selection};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Width of the sidebar.
const SIDEBAR_WIDTH: u16 = 25;

const SELECTED: Style = Style::new().add_modifier(Modifier::REVERSED);

/// The screen's regions for its size; `None` below 40×10.
struct Areas {
    sidebar: Option<Rect>,
    editor: Option<Rect>,
    status: Option<Rect>,
}

/// The width bands: sidebar at 120 columns or more (or while the explorer
/// has focus), status bar from 60, editor only below 60 — where a focused
/// explorer takes the whole width.
fn areas(area: Rect, explorer_focused: bool) -> Option<Areas> {
    if area.width < 40 || area.height < 10 {
        return None;
    }
    let (body, status) = if area.width >= 60 {
        let [body, status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        (body, Some(status))
    } else {
        (area, None)
    };
    let areas = if area.width < 60 && explorer_focused {
        Areas {
            sidebar: Some(body),
            editor: None,
            status,
        }
    } else if area.width >= 120 || explorer_focused {
        let [sidebar, editor] =
            Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(1)]).areas(body);
        Areas {
            sidebar: Some(sidebar),
            editor: Some(editor),
            status,
        }
    } else {
        Areas {
            sidebar: None,
            editor: Some(body),
            status,
        }
    };
    Some(areas)
}

pub(crate) fn draw(app: &mut App, frame: &mut Frame) {
    let Some(areas) = areas(frame.area(), app.focus == Focus::Explorer) else {
        draw_notice(frame);
        return;
    };
    if let Some(sidebar) = areas.sidebar {
        draw_sidebar(app, frame, sidebar);
    }
    if let Some(editor) = areas.editor {
        draw_editor_group(app, frame, editor);
    }
    if let Some(status) = areas.status {
        draw_status(app, frame, status);
    }
}

fn draw_notice(frame: &mut Frame) {
    let area = frame.area();
    let top = area.height.saturating_sub(2) / 2;
    let notice = Rect {
        y: area.y + top,
        height: (area.height - top).min(2),
        ..area
    };
    let text = vec![
        Line::from("Terminal too small"),
        Line::from("(40×10 needed)"),
    ];
    frame.render_widget(Paragraph::new(text).alignment(Alignment::Center), notice);
}

fn file_name(document: &Document) -> String {
    document
        .path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn draw_sidebar(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = Block::new().borders(Borders::RIGHT);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let buffer = frame.buffer_mut();
    let bold = Style::new().add_modifier(Modifier::BOLD);
    buffer.set_line(
        inner.x,
        inner.y,
        &Line::styled(" EXPLORER", bold),
        inner.width,
    );
    let Some(explorer) = app.workspace.explorer() else {
        let line = Line::from("  No folder opened");
        buffer.set_line(inner.x, inner.y + 1, &line, inner.width);
        return;
    };
    let root = explorer
        .root()
        .file_name()
        .map(|n| n.to_string_lossy().to_uppercase())
        .unwrap_or_default();
    buffer.set_line(
        inner.x,
        inner.y + 1,
        &Line::styled(format!(" {root}"), bold),
        inner.width,
    );
    let rows = explorer.rows();
    let height = usize::from(inner.height.saturating_sub(2));
    app.explorer_height = height.max(1);
    app.explorer_row = app.explorer_row.min(rows.len().saturating_sub(1));
    app.explorer_top = scroll(app.explorer_top, app.explorer_row, height);
    let active = app
        .workspace
        .active_document()
        .and_then(|id| app.workspace.document(id))
        .map(|d| d.path().to_owned());
    let focused = app.focus == Focus::Explorer;
    for (offset, row) in rows.iter().enumerate().skip(app.explorer_top).take(height) {
        let marker = match (row.is_dir, row.expanded) {
            (true, true) => "▾ ",
            (true, false) => "▸ ",
            (false, _) => "  ",
        };
        let text = format!(" {}{marker}{}", "  ".repeat(row.depth), row.name);
        let mut style = Style::new();
        if active.as_deref() == Some(row.path.as_path()) {
            style = style.add_modifier(Modifier::BOLD);
        }
        if focused && offset == app.explorer_row {
            style = style.patch(SELECTED);
        }
        let y = inner.y + 2 + to_u16(offset - app.explorer_top);
        let padded = format!("{text:<width$}", width = usize::from(inner.width));
        buffer.set_line(inner.x, y, &Line::styled(padded, style), inner.width);
    }
}

fn draw_status(app: &App, frame: &mut Frame, area: Rect) {
    let document = app.document().and_then(|id| app.workspace.document(id));
    let right = document.map_or_else(String::new, |document| {
        let (line, column) = app
            .document()
            .and_then(|id| app.workspace.status_line_column(id).ok())
            .unwrap_or((1, 1));
        let eol = match document.eol() {
            Eol::Lf => "LF",
            Eol::Crlf => "CRLF",
        };
        format!(
            "Ln {line}, Col {column}  {}  {eol}  {} ",
            document.encoding_name(),
            document.language_name()
        )
    });
    let mut left = document.map_or_else(String::new, |document| {
        let dirty = if document.is_dirty() { " ●" } else { "" };
        format!(" {}{dirty}", file_name(document))
    });
    if let Some(message) = &app.message {
        left = format!("{left}  {message}");
    }
    let width = usize::from(area.width);
    let room = width.saturating_sub(right.width());
    let left = truncate(&left, room);
    let gap = " ".repeat(room - left.width());
    let bar = Line::from(format!("{left}{gap}{right}"));
    frame.render_widget(Paragraph::new(bar).style(SELECTED), area);
}

/// `text` cut to at most `width` columns.
fn truncate(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

/// The tab bar on the first row, then the active document below it.
fn draw_editor_group(app: &mut App, frame: &mut Frame, area: Rect) {
    let [tabs, text] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    draw_tabs(app, frame.buffer_mut(), tabs);
    if let Some(id) = app.document() {
        draw_editor(app, frame, text, id);
        return;
    }
    let hint = vec![
        Line::from("No file open"),
        Line::from("Open one from the explorer: Ctrl+Shift+E or F6"),
    ];
    let top = text.height.saturating_sub(2) / 2;
    let hint_area = Rect {
        y: text.y + top,
        height: (text.height - top).min(2),
        ..text
    };
    frame.render_widget(Paragraph::new(hint).alignment(Alignment::Center), hint_area);
}

/// One cell per tab: its name, ● when dirty; the active tab reversed. The
/// row scrolls so the active tab stays visible.
fn draw_tabs(app: &App, buffer: &mut Buffer, area: Rect) {
    let active = app.document();
    let labels: Vec<(String, bool)> = app
        .workspace
        .tabs()
        .iter()
        .filter_map(|&id| app.workspace.document(id).map(|d| (id, d)))
        .map(|(id, document)| {
            let dirty = if document.is_dirty() { " ●" } else { "" };
            (
                format!(" {}{dirty} ", file_name(document)),
                Some(id) == active,
            )
        })
        .collect();
    let width = usize::from(area.width);
    let mut start = 0;
    let end_of_active = labels
        .iter()
        .take_while(|(_, is_active)| !is_active)
        .map(|(label, _)| label.width() + 1)
        .sum::<usize>()
        + labels
            .iter()
            .find(|(_, is_active)| *is_active)
            .map_or(0, |(label, _)| label.width());
    let mut skipped = 0;
    while end_of_active.saturating_sub(skipped) > width && start < labels.len() {
        skipped += labels[start].0.width() + 1;
        start += 1;
    }
    let mut spans = Vec::new();
    for (label, is_active) in labels.into_iter().skip(start) {
        let style = if is_active {
            SELECTED.add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw("│"));
    }
    buffer.set_line(area.x, area.y, &Line::from(spans), area.width);
}

/// What the editor needs to draw one row.
struct RowContext<'a> {
    document: &'a Document,
    selections: &'a [Selection],
    primary: Selection,
    tab_size: usize,
    left: usize,
    width: usize,
}

fn draw_editor(app: &mut App, frame: &mut Frame, area: Rect, id: DocumentId) {
    let tab_size = app.workspace.tab_size(id).unwrap_or(4);
    let view = app.views.get(&id).copied().unwrap_or_default();
    let Some(document) = app.workspace.document(id) else {
        return;
    };
    let lines = document.line_count();
    let area_width = usize::from(area.width);
    let gutter = (lines.to_string().len().max(3) + 2).min(area_width);
    let width = area_width - gutter;
    let height = usize::from(area.height);
    let cursor = document.cursor_position();
    let cursor_x = display_width(
        document.line(cursor.line).chars().take(cursor.column),
        tab_size,
    );
    let view = Viewport {
        top: scroll(view.top, cursor.line, height),
        left: scroll(view.left, cursor_x, width),
    };
    let context = RowContext {
        document,
        selections: document.selections(),
        primary: document.primary_selection(),
        tab_size,
        left: view.left,
        width,
    };
    let buffer = frame.buffer_mut();
    for (row, line) in (view.top..lines).take(height).enumerate() {
        let number_style = if line == cursor.line {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        };
        let number = Span::styled(format!("{:>w$} ", line + 1, w = gutter - 1), number_style);
        let mut spans = vec![number];
        spans.extend(row_spans(&context, line));
        buffer.set_line(area.x, area.y + to_u16(row), &Line::from(spans), area.width);
    }
    app.views.insert(id, view);
    app.page = height.saturating_sub(2).max(1);
    if width > 0 && app.focus == Focus::Editor {
        let x = area.x + to_u16(gutter + cursor_x - view.left);
        let y = area.y + to_u16(cursor.line - view.top);
        frame.set_cursor_position((x, y));
    }
}

/// The first visible index so that `target` is inside a window of `size`.
fn scroll(current: usize, target: usize, size: usize) -> usize {
    if size == 0 || target < current {
        target
    } else if target >= current + size {
        target + 1 - size
    } else {
        current
    }
}

/// The text cells of one line, clipped to the visible columns.
fn row_spans(context: &RowContext<'_>, line: usize) -> Vec<Span<'static>> {
    let document = context.document;
    let start = document.offset_of(Position::new(line, 0));
    let text = document.line(line);
    let mut cells = Cells::new(context.left, context.width);
    let mut end = start;
    for (offset, c) in (start..).zip(text.chars()) {
        cells.push(c, style_at(context, offset), context.tab_size);
        end = offset + 1;
    }
    let breaks = line + 1 < document.line_count();
    if let Some(style) = end_style(context, end, breaks) {
        cells.push(' ', style, context.tab_size);
    }
    cells.spans
}

/// Selected text and secondary cursors are drawn reversed; the primary
/// cursor is the terminal's own cursor.
fn style_at(context: &RowContext<'_>, offset: usize) -> Style {
    let selected = context.selections.iter().any(|s| {
        let range = s.range();
        range.start <= offset && offset < range.end
    });
    let secondary_cursor = context
        .selections
        .iter()
        .any(|s| s.is_empty() && s.head == offset && *s != context.primary);
    if selected || secondary_cursor {
        SELECTED
    } else {
        Style::new()
    }
}

/// A reversed cell after the text when the line break is selected or a
/// secondary cursor sits at the end of the line.
fn end_style(context: &RowContext<'_>, end: usize, breaks: bool) -> Option<Style> {
    let break_selected = breaks
        && context.selections.iter().any(|s| {
            let range = s.range();
            range.start <= end && end < range.end
        });
    let cursor = context
        .selections
        .iter()
        .any(|s| s.is_empty() && s.head == end && *s != context.primary);
    (break_selected || cursor).then_some(SELECTED)
}

/// How a char is drawn: its symbol and the columns it takes.
fn glyph(c: char, column: usize, tab_size: usize) -> (String, usize) {
    match c {
        '\t' => {
            let width = tab_size - column % tab_size;
            (" ".repeat(width), width)
        }
        '\u{0}'..='\u{1f}' => {
            let picture = char::from_u32(0x2400 + u32::from(c)).unwrap_or('?');
            (picture.to_string(), 1)
        }
        '\u{7f}' => ("\u{2421}".to_owned(), 1),
        '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}'
        | '\u{200e}'
        | '\u{200f}'
        | '\u{061c}'
        | '\u{85}'
        | '\u{2028}'
        | '\u{2029}' => (format!("[U+{:04X}]", u32::from(c)), 8),
        _ => (c.to_string(), c.width().unwrap_or(0)),
    }
}

/// Columns taken by `chars` from the start of a line.
fn display_width(chars: impl Iterator<Item = char>, tab_size: usize) -> usize {
    chars.fold(0, |column, c| column + glyph(c, column, tab_size).1)
}

/// Builds styled runs for the visible columns `left..left + width`.
struct Cells {
    left: usize,
    width: usize,
    column: usize,
    spans: Vec<Span<'static>>,
}

impl Cells {
    fn new(left: usize, width: usize) -> Self {
        Self {
            left,
            width,
            column: 0,
            spans: Vec::new(),
        }
    }

    fn push(&mut self, c: char, style: Style, tab_size: usize) {
        let (symbol, width) = glyph(c, self.column, tab_size);
        let start = self.column;
        self.column += width;
        let (visible_start, visible_end) = (self.left, self.left + self.width);
        if width == 0 {
            // A combining mark joins the char before it, if that one is shown.
            if start > visible_start && start <= visible_end {
                self.append(&symbol, style);
            }
            return;
        }
        if start >= visible_end || start + width <= visible_start {
            return;
        }
        if start >= visible_start && start + width <= visible_end {
            self.append(&symbol, style);
        } else {
            // Cut at an edge: wide chars become blanks, ASCII symbols are sliced.
            let from = visible_start.saturating_sub(start);
            let to = width.min(visible_end - start);
            let part = if symbol.is_ascii() && symbol.len() == width {
                symbol[from..to].to_owned()
            } else {
                " ".repeat(to - from)
            };
            self.append(&part, style);
        }
    }

    fn append(&mut self, text: &str, style: Style) {
        match self.spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push_str(text),
            _ => self.spans.push(Span::styled(text.to_owned(), style)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(text: &str, left: usize, width: usize) -> String {
        let mut cells = Cells::new(left, width);
        for c in text.chars() {
            cells.push(c, Style::new(), 4);
        }
        cells.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn tabs_wide_chars_and_invisibles_take_their_columns() {
        assert_eq!(visible("a\tb", 0, 80), "a   b");
        assert_eq!(visible("日本", 0, 80), "日本");
        assert_eq!(
            visible("日本", 1, 80),
            " 本",
            "a cut wide char becomes a blank"
        );
        assert_eq!(visible("a\u{0}b\u{7f}", 0, 80), "a\u{2400}b\u{2421}");
        assert_eq!(visible("x\u{202e}y", 0, 80), "x[U+202E]y");
        assert_eq!(visible("x\u{202e}y", 3, 4), "+202");
        assert_eq!(visible("e\u{301}x", 0, 80), "e\u{301}x");
        assert_eq!(display_width("\tab".chars(), 4), 6);
    }

    #[test]
    fn scrolling_keeps_the_target_in_view() {
        assert_eq!(scroll(0, 5, 10), 0);
        assert_eq!(scroll(0, 15, 10), 6);
        assert_eq!(scroll(8, 3, 10), 3);
        assert_eq!(scroll(4, 7, 0), 7);
    }

    #[test]
    fn a_focused_explorer_shows_at_any_width() {
        let narrow = areas(Rect::new(0, 0, 50, 12), true).expect("areas");
        assert!(
            narrow.editor.is_none(),
            "below 60 the explorer takes the width"
        );
        let medium = areas(Rect::new(0, 0, 100, 12), true).expect("areas");
        assert!(medium.sidebar.is_some() && medium.editor.is_some());
        assert!(
            areas(Rect::new(0, 0, 100, 12), false)
                .expect("areas")
                .sidebar
                .is_none()
        );
    }
}
