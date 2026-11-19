//! Drawing: width bands, sidebar, tabs, editor and status bar.
//!
//! Only the visible lines are read from the workspace. Tabs advance to the
//! next tab stop of `editor.tabSize`. Invisible characters are drawn as VS
//! Code draws them (`src/vs/editor/common/viewLayout/viewLineRenderer.ts:728-758`,
//! `:1199-1209`): C0 controls as control pictures (U+2400 + code), DEL as ␡,
//! and bidirectional formatting characters as `[U+XXXX]`. Unicode line and
//! paragraph separators and NEL, which do not end lines, are shown the same
//! way so they cannot hide in a terminal. Syntax highlights are asked of the
//! workspace for the visible lines only.
//!
//! Diagnostics are underlined over the range the workspace gives (an empty
//! one at a line's end takes one cell), and the gutter shows the line's most
//! severe problem as E, W or I in its first column.

use crate::app::{App, EditorHit, Focus, Hits, QUICK_ROWS, QuickItem, Viewport};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use rustode_workspace::{
    Document, DocumentId, Eol, Highlight, Marker, MarkerSeverity, Position, Selection, ServerState,
};
use std::collections::HashMap;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Width of the sidebar.
const SIDEBAR_WIDTH: u16 = 25;

/// The screen's regions for its size; `None` below 40×10.
struct Areas {
    sidebar: Option<Rect>,
    editor: Option<Rect>,
    status: Option<Rect>,
}

/// The width bands: sidebar at 120 columns or more (or while the sidebar
/// has focus), status bar from 60, editor only below 60 — where a focused
/// sidebar takes the whole width. The sidebar is `sidebar_width` wide.
fn areas(area: Rect, explorer_focused: bool, sidebar_width: u16) -> Option<Areas> {
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
            Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(1)]).areas(body);
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
    let theme = app.theme;
    app.hits = Hits::default();
    let screen = frame.area();
    frame.buffer_mut().set_style(screen, theme.editor);
    let sidebar_focused = matches!(
        app.focus,
        Focus::Explorer | Focus::Extensions | Focus::Search
    );
    let sidebar_width = if app.search.shown {
        crate::search::WIDTH
    } else if app.extensions.shown {
        crate::extensions::WIDTH
    } else {
        SIDEBAR_WIDTH
    };
    let Some(areas) = areas(screen, sidebar_focused, sidebar_width) else {
        draw_notice(frame);
        return;
    };
    if let Some(sidebar) = areas.sidebar {
        frame.buffer_mut().set_style(sidebar, theme.sidebar);
        if app.search.shown {
            crate::search::draw(app, frame, sidebar);
        } else if app.extensions.shown {
            crate::extensions::draw(app, frame, sidebar);
        } else {
            draw_sidebar(app, frame, sidebar);
        }
    }
    if let Some(editor) = areas.editor {
        let editor = if app.problems.visible {
            let panel = crate::problems::height(editor.height);
            let [editor, problems] =
                Layout::vertical([Constraint::Min(1), Constraint::Length(panel)]).areas(editor);
            crate::problems::draw(app, frame, problems);
            editor
        } else {
            editor
        };
        draw_editor_group(app, frame, editor);
        if app.find_visible {
            draw_find(app, frame, editor);
        }
    }
    if let Some(status) = areas.status {
        frame.buffer_mut().set_style(status, theme.status);
        draw_status(app, frame, status);
    }
    crate::suggest::draw(app, frame);
    if app.quick.is_some() {
        draw_quick(app, frame);
    }
}

/// The quick input box, near the top in the middle like VS Code's: the
/// text, then the matching rows with matched chars in bold.
fn draw_quick(app: &mut App, frame: &mut Frame) {
    let items = app.quick_items();
    let Some(quick) = app.quick.as_ref() else {
        return;
    };
    let area = frame.area();
    let width = area.width.saturating_sub(4).min(80);
    let rows = items.len().min(QUICK_ROWS);
    let height = to_u16(rows + 3).min(area.height.saturating_sub(1));
    let outer = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + 1,
        width,
        height,
    };
    let theme = app.theme;
    frame.render_widget(Clear, outer);
    frame.buffer_mut().set_style(outer, theme.quick);
    let block = Block::bordered().border_style(theme.quick_border);
    let inner = block.inner(outer);
    frame.render_widget(block, outer);
    let selected = quick.selected.min(items.len().saturating_sub(1));
    let first = scroll(0, selected, rows);
    let text_width = quick.text.width();
    let buffer = frame.buffer_mut();
    buffer.set_line(
        inner.x,
        inner.y,
        &Line::from(format!(" {}", quick.text)),
        inner.width,
    );
    for (row, (index, item)) in items.iter().enumerate().skip(first).take(rows).enumerate() {
        let line = quick_line(item, usize::from(inner.width), index == selected, &theme);
        buffer.set_line(inner.x, inner.y + 1 + to_u16(row), &line, inner.width);
    }
    let cursor_x = (inner.x + 1 + to_u16(text_width)).min(inner.right().saturating_sub(1));
    frame.set_cursor_position((cursor_x, inner.y));
}

/// The find widget at the top right of the editor, like VS Code's: the
/// find field, the Aa / ab / .* toggles (shown selected when on), the match
/// status, and the replace field in replace mode.
fn draw_find(app: &App, frame: &mut Frame, area: Rect) {
    let Some(find) = app.find.as_ref() else {
        return;
    };
    let theme = app.theme;
    let width = area.width.min(64);
    let rows: u16 = if find.replace_mode { 2 } else { 1 };
    let outer = Rect {
        x: area.right() - width,
        y: area.y + 1,
        width,
        height: (rows + 2).min(area.height.saturating_sub(1)),
    };
    frame.render_widget(Clear, outer);
    frame.buffer_mut().set_style(outer, theme.quick);
    let block = Block::bordered().border_style(theme.quick_border);
    let inner = block.inner(outer);
    frame.render_widget(block, outer);
    let toggle = |on: bool, label: &str| {
        Span::styled(
            format!(" {label} "),
            if on {
                theme.quick_selected
            } else {
                theme.description
            },
        )
    };
    let status = app.find_status();
    let right = vec![
        toggle(find.options.match_case, "Aa"),
        toggle(find.options.whole_word, "ab"),
        toggle(find.options.regex, ".*"),
        Span::styled(format!(" {status}"), theme.description),
    ];
    let right_width: usize = right.iter().map(|s| s.content.width()).sum();
    let field_width = usize::from(inner.width).saturating_sub(right_width + 1);
    let find_text = truncate(&format!(" Find: {}", find.query), field_width);
    let mut spans = vec![Span::raw(format!("{find_text:<field_width$} "))];
    spans.extend(right);
    let buffer = frame.buffer_mut();
    buffer.set_line(inner.x, inner.y, &Line::from(spans), inner.width);
    let replace_text = truncate(
        &format!(" Replace: {}", find.replacement),
        usize::from(inner.width),
    );
    if find.replace_mode {
        buffer.set_line(
            inner.x,
            inner.y + 1,
            &Line::from(replace_text.clone()),
            inner.width,
        );
    }
    let (row, text) = if find.in_replace {
        (1, replace_text)
    } else {
        (0, find_text)
    };
    let x = (inner.x + to_u16(text.width())).min(inner.right().saturating_sub(1));
    frame.set_cursor_position((x, inner.y + row));
}

/// One row of the quick input list, padded to `width`.
fn quick_line(item: &QuickItem, width: usize, selected: bool, theme: &Theme) -> Line<'static> {
    let dim = theme.description;
    let highlight = theme.highlight;
    let (mut spans, right) = match item {
        QuickItem::File(file) => {
            let mut spans = highlighted(
                &format!(" {}", file.label),
                &shift(&file.score.label),
                highlight,
            );
            spans.push(Span::styled(format!("  {}", file.description), dim));
            (spans, String::new())
        }
        QuickItem::Command(found) => (
            highlighted(
                &format!(" {}", found.command.label),
                &shift(&found.positions),
                highlight,
            ),
            found.keybinding.clone().unwrap_or_default(),
        ),
        QuickItem::Line { line, column } => {
            let text = match column {
                Some(column) => format!(" Go to line {line}, column {column}"),
                None => format!(" Go to line {line}"),
            };
            (vec![Span::raw(text)], String::new())
        }
        QuickItem::Hint(hint) => (vec![Span::styled(format!(" {hint}"), dim)], String::new()),
        QuickItem::Language(pick) => {
            let mut spans = highlighted(
                &format!(" {}", pick.label),
                &shift(&pick.positions),
                highlight,
            );
            spans.push(Span::styled(format!("  {}", pick.description), dim));
            (spans, pick.extensions.clone())
        }
    };
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    let right = truncate(&right, width.saturating_sub(used + 2));
    let gap = width.saturating_sub(used + right.width() + 1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(format!("{right} "), dim));
    let mut line = Line::from(spans);
    if selected {
        line = line.patch_style(theme.quick_selected);
    }
    line
}

/// Positions moved past the leading space of a row.
fn shift(positions: &[usize]) -> Vec<usize> {
    positions.iter().map(|p| p + 1).collect()
}

/// `text` with the chars at `positions` highlighted.
pub(crate) fn highlighted(text: &str, positions: &[usize], highlight: Style) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (index, c) in text.chars().enumerate() {
        let style = if positions.contains(&index) {
            highlight
        } else {
            Style::new()
        };
        match spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push(c),
            _ => spans.push(Span::styled(c.to_string(), style)),
        }
    }
    spans
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
        .map_or_else(Default::default, |name| name.to_string_lossy().into_owned())
}

pub(crate) fn to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn draw_sidebar(app: &mut App, frame: &mut Frame, area: Rect) {
    let theme = app.theme;
    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_style(theme.separator);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let buffer = frame.buffer_mut();
    let bold = theme.sidebar_header;
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
        .map_or_else(Default::default, |n| n.to_string_lossy().to_uppercase());
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
    if app.explorer_revealed != Some(app.explorer_row) {
        app.explorer_top = scroll(app.explorer_top, app.explorer_row, height);
        app.explorer_revealed = Some(app.explorer_row);
    }
    app.explorer_top = app.explorer_top.min(rows.len().saturating_sub(1));
    app.hits.explorer = Some(Rect {
        y: inner.y + 2,
        height: to_u16(height),
        ..inner
    });
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
            style = style.patch(theme.explorer_active);
        }
        if focused && offset == app.explorer_row {
            style = style.patch(theme.explorer_selected);
        }
        let y = inner.y + 2 + to_u16(offset - app.explorer_top);
        let padded = format!("{text:<width$}", width = usize::from(inner.width));
        buffer.set_line(inner.x, y, &Line::styled(padded, style), inner.width);
    }
}

fn draw_status(app: &mut App, frame: &mut Frame, area: Rect) {
    let document = app.document().and_then(|id| app.workspace.document(id));
    let server = app
        .document()
        .and_then(|id| app.workspace.language_server(id))
        .map_or_else(String::new, |status| match status.state {
            ServerState::Running => format!("{}  ", status.name),
            state => format!("{} ({})  ", status.name, state.label()),
        });
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
            "Ln {line}, Col {column}  {}  {eol}  {server}{} ",
            document.encoding_name(),
            document.language_name()
        )
    });
    // The counts show once a language server is configured: until then
    // nothing can report a problem.
    let counts =
        (!app.workspace.language_servers().is_empty()).then(|| crate::problems::counts_label(app));
    let mut left = counts
        .as_ref()
        .map_or_else(String::new, |c| format!(" {c} "));
    app.hits.problems_status = counts
        .as_ref()
        .map(|c| (area.y, area.x, area.x.saturating_add(to_u16(c.width() + 2))));
    left.push_str(&document.map_or_else(String::new, |document| {
        let dirty = if document.is_dirty() { " ●" } else { "" };
        format!(" {}{dirty}", file_name(document))
    }));
    if let Some(message) = &app.message {
        left = format!("{left}  {message}");
    }
    let width = usize::from(area.width);
    app.hits.language_status = document.map(|d| {
        let name = to_u16(d.language_name().width() + 1);
        (
            area.y,
            (area.x + area.width).saturating_sub(name + 1),
            area.x + area.width,
        )
    });
    let room = width.saturating_sub(right.width());
    let left = truncate(&left, room);
    let gap = " ".repeat(room - left.width());
    let bar = Line::from(format!("{left}{gap}{right}"));
    frame.render_widget(Paragraph::new(bar).style(app.theme.status), area);
}

/// `text` cut to at most `width` columns.
pub(crate) fn truncate(text: &str, width: usize) -> String {
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
    app.hits.tabs = draw_tabs(app, frame.buffer_mut(), tabs);
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
fn draw_tabs(app: &App, buffer: &mut Buffer, area: Rect) -> Vec<(u16, u16, u16, DocumentId)> {
    let active = app.document();
    let ids: Vec<DocumentId> = app
        .workspace
        .tabs()
        .iter()
        .copied()
        .filter(|&id| app.workspace.document(id).is_some())
        .collect();
    let mut tabs = Vec::new();
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
    let theme = app.theme;
    buffer.set_style(area, theme.tab_bar);
    let mut spans = Vec::new();
    let mut x = area.x;
    for ((label, is_active), id) in labels.into_iter().zip(ids).skip(start) {
        let style = if is_active {
            theme.tab_active
        } else {
            theme.tab_inactive
        };
        let end = x.saturating_add(to_u16(label.width()));
        tabs.push((area.y, x, end, id));
        x = end.saturating_add(1);
        spans.push(Span::styled(label, style));
        spans.push(Span::styled("│", theme.separator));
    }
    buffer.set_line(area.x, area.y, &Line::from(spans), area.width);
    tabs
}

/// What the editor needs to draw one row.
struct RowContext<'a> {
    document: &'a Document,
    selections: &'a [Selection],
    primary: Selection,
    tab_size: usize,
    left: usize,
    width: usize,
    theme: Theme,
    /// The highlights of the visible lines, sorted.
    highlights: Vec<Highlight>,
    /// The diagnostics touching the visible lines, with their underline.
    markers: Vec<(Marker, Style)>,
}

/// The diagnostics of document `id` on lines `top..top + height`.
fn visible_markers(
    workspace: &rustode_workspace::Workspace,
    theme: &Theme,
    id: DocumentId,
    top: usize,
    height: usize,
) -> Vec<(Marker, Style)> {
    let Some(document) = workspace.document(id) else {
        return Vec::new();
    };
    let first = document.offset_of(Position::new(top, 0));
    let last = document.offset_of(Position::new(top + height, 0));
    workspace
        .markers(id)
        .into_iter()
        .filter(|m| m.range.end >= first && m.range.start <= last)
        .map(|m| {
            let style = theme.marker(m.severity);
            (m, style)
        })
        .collect()
}

/// The line number, with the severity letter of the line's most severe
/// problem in the first column.
fn gutter_spans(
    theme: &Theme,
    letter: Option<MarkerSeverity>,
    line: usize,
    gutter: usize,
    active: bool,
) -> Vec<Span<'static>> {
    let style = if active {
        theme.gutter_active
    } else {
        theme.gutter
    };
    match letter {
        Some(severity) if gutter >= 3 => {
            let (letter, letter_style) = theme.letter(severity);
            vec![
                Span::styled(letter, letter_style),
                Span::styled(format!("{:>w$} ", line + 1, w = gutter - 2), style),
            ]
        }
        _ => vec![Span::styled(
            format!("{:>w$} ", line + 1, w = gutter - 1),
            style,
        )],
    }
}

/// The most severe problem starting on each line, hints left out.
fn gutter_letters(
    document: &Document,
    markers: &[(Marker, Style)],
) -> HashMap<usize, MarkerSeverity> {
    let mut letters = HashMap::new();
    for (marker, _) in markers {
        if marker.severity == MarkerSeverity::Hint {
            continue;
        }
        let line = document.position_of(marker.range.start).line;
        let worst = letters.entry(line).or_insert(marker.severity);
        *worst = (*worst).max(marker.severity);
    }
    letters
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
    // Follow the cursor only when it moved, so wheel scrolling sticks.
    let view = if view.revealed == Some(cursor) {
        Viewport {
            top: view.top.min(lines.saturating_sub(1)),
            ..view
        }
    } else {
        Viewport {
            top: scroll(view.top, cursor.line, height),
            left: scroll(view.left, cursor_x, width),
            revealed: Some(cursor),
        }
    };
    let markers = visible_markers(&app.workspace, &app.theme, id, view.top, height);
    let letters = gutter_letters(document, &markers);
    let context = RowContext {
        document,
        selections: document.selections(),
        primary: document.primary_selection(),
        tab_size,
        left: view.left,
        width,
        theme: app.theme,
        highlights: document.highlights(view.top..view.top + height),
        markers,
    };
    let buffer = frame.buffer_mut();
    for (row, line) in (view.top..lines).take(height).enumerate() {
        let letter = letters.get(&line).copied();
        let mut spans = gutter_spans(&app.theme, letter, line, gutter, line == cursor.line);
        spans.extend(row_spans(&context, line));
        buffer.set_line(area.x, area.y + to_u16(row), &Line::from(spans), area.width);
    }
    app.views.insert(id, view);
    app.page = height.saturating_sub(2).max(1);
    app.hits.editor = Some(EditorHit {
        area,
        gutter: to_u16(gutter),
        id,
        tab_size,
    });
    let visible = cursor.line >= view.top
        && cursor.line < view.top + height
        && cursor_x >= view.left
        && cursor_x < view.left + width;
    if visible && app.focus == Focus::Editor {
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
        let style = style_at(context, offset);
        let style = marker_style(context, offset).map_or(style, |marker| style.patch(marker));
        cells.push(c, style, context.tab_size);
        end = offset + 1;
    }
    let breaks = line + 1 < document.line_count();
    let marker = marker_style(context, end);
    match (end_style(context, end, breaks), marker) {
        (Some(style), marker) => {
            cells.push(
                ' ',
                style.patch(marker.unwrap_or_default()),
                context.tab_size,
            );
        }
        (None, Some(marker)) => cells.push(' ', marker, context.tab_size),
        (None, None) => {}
    }
    cells.spans
}

/// The underline of the most severe diagnostic at `offset`: one covering
/// it, or an empty one there.
fn marker_style(context: &RowContext<'_>, offset: usize) -> Option<Style> {
    context
        .markers
        .iter()
        .filter(|(m, _)| {
            (m.range.start <= offset && offset < m.range.end)
                || (m.range.is_empty() && m.range.start == offset)
        })
        .max_by_key(|(m, _)| m.severity)
        .map(|(_, style)| *style)
}

/// The token style of the char at `offset`, plain outside highlights.
fn token_at(context: &RowContext<'_>, offset: usize) -> Style {
    let highlights = &context.highlights;
    let index = highlights.partition_point(|h| h.range.end <= offset);
    match highlights.get(index) {
        Some(h) if h.range.start <= offset => context.theme.token(h.kind),
        _ => Style::new(),
    }
}

/// Text takes its token style; selected text and secondary cursors get the
/// theme's styles; the primary cursor is the terminal's own cursor.
fn style_at(context: &RowContext<'_>, offset: usize) -> Style {
    let selected = context.selections.iter().any(|s| {
        let range = s.range();
        range.start <= offset && offset < range.end
    });
    let secondary_cursor = context
        .selections
        .iter()
        .any(|s| s.is_empty() && s.head == offset && *s != context.primary);
    let token = token_at(context, offset);
    if secondary_cursor {
        context.theme.secondary_cursor
    } else if selected {
        context.theme.selected(token)
    } else {
        token
    }
}

/// A highlighted cell after the text when the line break is selected or a
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
    if cursor {
        Some(context.theme.secondary_cursor)
    } else {
        break_selected.then_some(context.theme.selection)
    }
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

/// The char column a click at display column `display` lands on: before a
/// char when its left half is hit, after it for the right half.
pub(crate) fn column_at(text: &str, display: usize, tab_size: usize) -> usize {
    let mut column = 0;
    for (index, c) in text.chars().enumerate() {
        let width = glyph(c, column, tab_size).1;
        if width > 0 && display < column + width.div_ceil(2) {
            return index;
        }
        column += width;
    }
    text.chars().count()
}

/// Columns taken by `chars` from the start of a line.
pub(crate) fn display_width(chars: impl Iterator<Item = char>, tab_size: usize) -> usize {
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
        let narrow = areas(Rect::new(0, 0, 50, 12), true, SIDEBAR_WIDTH).expect("areas");
        assert!(
            narrow.editor.is_none(),
            "below 60 the explorer takes the width"
        );
        let medium = areas(Rect::new(0, 0, 100, 12), true, SIDEBAR_WIDTH).expect("areas");
        assert!(medium.sidebar.is_some() && medium.editor.is_some());
        assert!(
            areas(Rect::new(0, 0, 100, 12), false, SIDEBAR_WIDTH)
                .expect("areas")
                .sidebar
                .is_none()
        );
    }
}
