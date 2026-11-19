//! The editor widget. It draws only the visible lines, asking the workspace
//! for each of them (`Document::line`), so the cost of a frame does not
//! grow with the document; egui's `TextEdit` would copy the whole text every
//! frame. Keyboard and mouse input become workspace commands.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//! the line height is `round(1.35 × 14)` = 19 px on Linux
//! (`src/vs/editor/common/config/fontInfo.ts:14`, `:33-46`, `:230-240`);
//! Page Up and Page Down move by the full lines in view minus two
//! (`src/vs/editor/common/cursorCommon.ts:129`); the colors are Dark
//! Modern's (`extensions/theme-defaults/themes/dark_modern.json:33-44`) with
//! the default selection and caret colors
//! (`src/vs/platform/theme/common/colors/editorColors.ts:121-123`,
//! `src/vs/editor/common/core/editorColorRegistry.ts:22`). While the find
//! widget is shown, every match in view is highlighted and the current one
//! stands out, in Dark Modern's `editor.findMatchBackground` (`dark_modern.json:34`)
//! and the default `editor.findMatchHighlightBackground`
//! (`src/vs/platform/theme/common/colors/editorColors.ts:148-158`).
//!
//! Syntax highlights are asked of the workspace for the visible lines only,
//! and each line is laid out in colored sections, with Dark Modern's token
//! colors (`TokenKind::dark_modern`).
//!
//! Diagnostics are squiggled under their range, an empty one one char
//! wide, in their severity's color (see `crate::problems`).
//!
//! The caret does not blink, so an idle window draws no frames, and as in
//! VS Code it is hidden while the editor does not have focus.
//!
//! **Sharp at any scale.** Every line is a whole number of physical pixels
//! tall (19 points rounded to pixels: 24 pixels at 1.25, 29 at 1.5) and
//! starts on a pixel boundary, as does the text area, so lines, selections
//! and carets tile with no seams and every line's glyphs sit the same way;
//! egui snaps the glyphs themselves to pixels. VS Code keeps 19 CSS pixels
//! and leaves the snapping to Chromium's layout. The caret is VS Code's: 2
//! points rounded down to whole pixels, at least one, and 1 point left of
//! the character boundary when it is at least 2 points wide and not at the
//! line's start (`computeScreenAwareSize`, `src/vs/base/browser/dom.ts:1609-1612`;
//! `src/vs/editor/browser/viewParts/viewCursors/viewCursor.ts:182-199`).

use crate::keys::{self, Command};
use crate::minimap;
use crate::suggest;
use crate::text::DisplayLine;
use eframe::egui::{
    self, Align2, Color32, EventFilter, FontId, Id, Key, Pos2, Rect, Response, Sense, Ui, Vec2,
    WidgetInfo, WidgetType, accesskit,
    emath::{GuiRounding as _, TSTransform},
    text::{CCursor, CCursorRange, Galley, LayoutJob, TextFormat},
};
use rustode_workspace::{
    CompletionMove, Document, DocumentId, Highlight, Marker, Position, Range, Selection, TokenKind,
    Workspace,
};
use std::sync::Arc;

/// Font size of the text, in points.
pub const FONT_SIZE: f32 = 14.0;
/// Height of one line of text at a scale factor of 1, in points; see
/// [`line_height`] for other scales.
pub const LINE_HEIGHT: f32 = 19.0;

const BACKGROUND: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const LINE_NUMBER: Color32 = Color32::from_rgb(0x6e, 0x76, 0x81);
const ACTIVE_LINE_NUMBER: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const SELECTION: Color32 = Color32::from_rgb(0x26, 0x4f, 0x78);
const CARET: Color32 = Color32::from_rgb(0xae, 0xaf, 0xad);
/// Dark Modern's `editor.findMatchBackground`, the current match.
const FIND_MATCH: Color32 = Color32::from_rgb(0x9e, 0x6a, 0x03);
/// The default `editor.findMatchHighlightBackground`, `#EA5C0055`: the other
/// matches.
const FIND_MATCH_HIGHLIGHT: Color32 = Color32::from_rgba_unmultiplied_const(0xea, 0x5c, 0x00, 0x55);
/// The caret's width before it is rounded to pixels, in points.
const CARET_WIDTH: f32 = 2.0;
/// Margin kept between the caret and the right edge when it is revealed.
const REVEAL_MARGIN: f32 = 30.0;

/// The id of the editor widget: one widget shows the active tab's document.
pub(crate) fn id() -> Id {
    Id::new("rustode-editor")
}

/// The height of one line at `pixels_per_point` physical pixels per point:
/// [`LINE_HEIGHT`] rounded to whole pixels, in points.
#[must_use]
pub fn line_height(pixels_per_point: f32) -> f32 {
    (LINE_HEIGHT * pixels_per_point).round().max(1.0) / pixels_per_point
}

/// VS Code's caret width at `pixels_per_point`: [`CARET_WIDTH`] rounded
/// down to whole pixels, at least one, in points.
fn caret_width(pixels_per_point: f32) -> f32 {
    (CARET_WIDTH * pixels_per_point).floor().max(1.0) / pixels_per_point
}

/// What the editor keeps between frames: where it is scrolled to.
///
/// The vertical position is a line plus a fraction of it, never a point
/// offset from the top, so it stays exact in files of any length.
#[derive(Debug, Default)]
pub(crate) struct View {
    /// The line at the top.
    top: usize,
    /// Points of `top` hidden above the view, from 0 to below a line.
    offset: f32,
    /// Points scrolled to the right.
    left: f32,
    /// The selection anchor while the primary button drags.
    drag_anchor: Option<usize>,
    /// Scroll the caret into view on the next frame: set when a command from
    /// outside the editor moved it (the quick input, the find widget, going
    /// to a problem).
    reveal: bool,
    /// The minimap's drag and cached blocks.
    minimap: crate::minimap::State,
}

impl View {
    /// Scrolls the primary caret into view on the next frame.
    pub(crate) fn reveal(&mut self) {
        self.reveal = true;
    }

    /// How far the view is scrolled, in lines.
    fn scroll_lines(&self, line_height: f32) -> f32 {
        float(self.top) + self.offset / line_height
    }

    /// Scrolls to `lines` lines from the top, on a whole pixel.
    fn set_scroll_lines(&mut self, lines: f32, line_height: f32, pixels_per_point: f32) {
        let lines = lines.max(0.0);
        self.top = count(lines.floor());
        self.offset = ((lines - lines.floor()) * line_height)
            .round_to_pixels(pixels_per_point)
            .clamp(0.0, line_height - 1.0 / pixels_per_point);
    }
}

/// What a frame's input did.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// Whether a keyboard command ran.
    pub(crate) changed: bool,
    /// Why the last failed command failed, for the status bar.
    pub(crate) error: Option<String>,
}

/// Where things go this frame, and how tabs are drawn.
struct Layout {
    gutter: Rect,
    text: Rect,
    font: FontId,
    char_width: f32,
    tab_size: usize,
    /// One line's height, a whole number of pixels.
    line_height: f32,
    pixels_per_point: f32,
}

impl Layout {
    fn new(ui: &Ui, rect: Rect, line_count: usize, tab_size: usize) -> Self {
        let font = FontId::monospace(FONT_SIZE);
        let char_width = char_width(ui);
        let digits = u8::try_from(line_count.max(1).ilog10() + 1).unwrap_or(u8::MAX);
        let gutter_width = char_width * (f32::from(digits.max(3)) + 2.0);
        let pixels_per_point = ui.pixels_per_point();
        let split = (rect.left() + gutter_width).round_to_pixels(pixels_per_point);
        let (gutter, text) = rect.split_left_right_at_x(split);
        Self {
            gutter,
            text,
            font,
            char_width,
            tab_size,
            line_height: line_height(pixels_per_point),
            pixels_per_point,
        }
    }

    /// Lines Page Up and Page Down move by.
    fn page(&self) -> usize {
        whole_lines(self.text.height(), self.line_height)
            .saturating_sub(2)
            .max(1)
    }

    /// Where the view's top line and left edge are drawn: on a pixel.
    fn origin(&self, view: &View) -> Pos2 {
        Pos2::new(self.text.left() - view.left, self.text.top() - view.offset)
            .round_to_pixels(self.pixels_per_point)
    }
}

/// One char's width in the editor's font.
fn char_width(ui: &Ui) -> f32 {
    let font = FontId::monospace(FONT_SIZE);
    ui.fonts_mut(|fonts| fonts.glyph_width(&font, ' '))
}

/// Splits the editor's `area` into the text's and, when the minimap is on
/// and fits, its own.
fn split_minimap(
    ui: &Ui,
    workspace: &Workspace,
    area: Rect,
) -> (Rect, Option<(Rect, minimap::Metrics)>) {
    let ppp = ui.pixels_per_point();
    let metrics = workspace
        .minimap_enabled()
        .then(|| minimap::Metrics::new(ppp, char_width(ui), area.width()))
        .flatten();
    match metrics {
        Some(metrics) => {
            let (text, bar) = minimap::split(area, metrics.width, ppp);
            (text, Some((bar, metrics)))
        }
        None => (area, None),
    }
}

/// Runs the minimap's input, which may scroll the view, and paints it.
fn show_minimap(
    ui: &Ui,
    workspace: &Workspace,
    document: DocumentId,
    view: &mut View,
    layout: &Layout,
    (bar, metrics): (Rect, minimap::Metrics),
) {
    let Some(doc) = workspace.document(document) else {
        return;
    };
    let scroll = view.scroll_lines(layout.line_height);
    let placed = minimap_layout(metrics, doc, layout, bar, scroll);
    let scroll =
        minimap::interact(ui, bar, &placed, scroll, &mut view.minimap).map_or(scroll, |to| {
            view.set_scroll_lines(to, layout.line_height, layout.pixels_per_point);
            view.scroll_lines(layout.line_height)
        });
    let placed = minimap_layout(metrics, doc, layout, bar, scroll);
    let paint = minimap::Paint {
        workspace,
        document,
        rect: bar,
        metrics,
        layout: placed,
        scroll,
    };
    minimap::paint(ui, &paint, &mut view.minimap);
}

/// Where the minimap's slider and lines are for the editor `layout`
/// scrolled `scroll` lines.
fn minimap_layout(
    metrics: minimap::Metrics,
    doc: &Document,
    layout: &Layout,
    bar: Rect,
    scroll: f32,
) -> minimap::Layout {
    minimap::Layout::new(
        metrics,
        minimap::Inputs {
            line_count: doc.line_count(),
            line_height: layout.line_height,
            viewport: layout.text.height(),
            scroll,
            height: bar.height(),
            pixels_per_point: layout.pixels_per_point,
        },
    )
}

/// A whole, non-negative float as a count.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // float to int casts saturate
pub(crate) fn count(value: f32) -> usize {
    value.max(0.0) as usize
}

/// A count of lines or columns in view (at most a few thousand) as a float.
#[allow(clippy::cast_precision_loss)] // exact below 2^24
pub(crate) fn float(count: usize) -> f32 {
    count as f32
}

/// Whole lines of `line_height` in `points`.
fn whole_lines(points: f32, line_height: f32) -> usize {
    count((points / line_height + 1e-4).floor())
}

/// Lines of `line_height` needed to cover `points`, the last one perhaps in
/// part.
fn lines_covering(points: f32, line_height: f32) -> usize {
    count((points / line_height - 1e-3).ceil())
}

/// One visible line, as drawn.
struct Row {
    line: usize,
    /// Screen y of the line's top.
    top: f32,
    /// The line's text, from the workspace.
    text: String,
    display: DisplayLine,
    galley: Arc<Galley>,
}

impl Row {
    /// Screen x of `column`, past the cut estimated with the char width.
    fn x(&self, layout: &Layout, view: &View, column: usize) -> f32 {
        let drawn = column.min(self.display.columns());
        let cursor = CCursor::new(self.display.display_index(drawn));
        let past_cut = float(column - drawn) * layout.char_width;
        layout.origin(view).x + self.galley.pos_from_cursor(cursor).min.x + past_cut
    }
}

/// The part of the document in view, laid out for this frame.
struct Screen<'a> {
    doc: &'a Document,
    layout: &'a Layout,
    view: &'a View,
    rows: Vec<Row>,
    /// The document's diagnostics, the most severe last.
    markers: Vec<Marker>,
    /// Find matches to highlight, sorted (`find.rs`).
    matches: &'a [Range],
}

impl Screen<'_> {
    /// The top left of the char at `offset`, if its line is in view.
    fn point_of(&self, offset: usize) -> Option<Pos2> {
        let position = self.doc.position_of(offset);
        let row = self.rows.iter().find(|row| row.line == position.line)?;
        Some(Pos2::new(
            row.x(self.layout, self.view, position.column),
            row.top,
        ))
    }

    /// The char under `pos`, when `pos` is over text: not past the end of its
    /// line, nor outside the text area.
    fn char_at(&self, ui: &Ui, pos: Pos2) -> Option<usize> {
        if !self.layout.text.contains(pos) {
            return None;
        }
        let offset = offset_at(ui, self.doc, self.layout, self.view, pos);
        let position = self.doc.position_of(offset);
        let row = self.rows.iter().find(|row| row.line == position.line)?;
        let end = row.x(self.layout, self.view, row.text.chars().count());
        (position.column < row.text.chars().count() && pos.x < end).then_some(offset)
    }
}

impl Row {
    fn top_of(layout: &Layout, view: &View, line: usize) -> f32 {
        layout.origin(view).y + float(line - view.top) * layout.line_height
    }
}

/// What is shown with the editor: its accessible name and the find matches.
pub(crate) struct Shown<'a> {
    /// The accessible name of the editor.
    pub label: &'a str,
    /// Find matches to highlight, sorted.
    pub matches: &'a [Range],
}

/// Shows the editor in `ui`, running the commands its input asks for, with
/// the find matches highlighted.
pub(crate) fn show(
    ui: &mut Ui,
    workspace: &mut Workspace,
    document: DocumentId,
    view: &mut View,
    overlay: &mut suggest::State,
    shown: &Shown<'_>,
) -> Outcome {
    let Shown { label, matches } = *shown;
    let mut outcome = Outcome::default();
    let area = ui.available_rect_before_wrap();
    let Some(line_count) = workspace.document(document).map(Document::line_count) else {
        return outcome;
    };
    let (rect, minimap_area) = split_minimap(ui, workspace, area);
    let response = ui.interact(rect, id(), Sense::click_and_drag());
    let tab_size = workspace.tab_size(document).unwrap_or(4);
    let layout = Layout::new(ui, rect, line_count, tab_size);
    if response.is_pointer_button_down_on() {
        response.request_focus();
    }
    if ui.memory(|memory| memory.has_focus(id())) {
        let arrived = response.gained_focus();
        let keys = suggest::keys(ui, workspace, overlay);
        outcome.changed |= keys.acted;
        outcome.error = keys.error;
        keyboard(
            ui,
            workspace,
            document,
            layout.page(),
            arrived,
            &mut outcome,
        );
    } else if !suggest::pointer_on_list(ui) {
        workspace.dismiss_completion();
    }
    pointer(ui, &response, workspace, document, &layout, view);
    let Some(doc) = workspace.document(document) else {
        return outcome;
    };
    let over_minimap = minimap_area.is_some_and(|(bar, _)| {
        ui.input(|input| input.pointer.hover_pos())
            .is_some_and(|pos| bar.contains(pos))
    });
    if response.contains_pointer() || over_minimap {
        wheel(ui, view, doc.line_count(), layout.line_height);
    }
    if std::mem::take(&mut view.reveal) || outcome.changed {
        reveal(ui, doc, &layout, view);
    }
    if let Some(area) = minimap_area {
        show_minimap(ui, workspace, document, view, &layout, area);
    }
    let mut markers = workspace.markers(document);
    markers.sort_by_key(|m| m.severity);
    let screen = Screen {
        doc,
        layout: &layout,
        view,
        rows: rows(ui, doc, &layout, view),
        markers,
        matches,
    };
    paint(ui, &screen);
    expose(ui, &response, &screen, label);
    let (pointer, clicked) = overlays(ui, &response, workspace, document, &screen, overlay);
    match pointer {
        suggest::Pointer::Idle => {}
        suggest::Pointer::Hide => workspace.hide_hover(),
        suggest::Pointer::Ask(offset) => drop(workspace.request_hover(document, offset)),
    }
    if let Some(row) = clicked {
        workspace.completion_move(CompletionMove::To(row));
        outcome.error = workspace.completion_accept().err().map(|e| e.to_string());
        outcome.changed = true;
    }
    outcome
}

/// The hover and suggest overlays over the painted text: what the pointer
/// wants of the hover, and the list row that was clicked.
fn overlays(
    ui: &Ui,
    response: &Response,
    workspace: &Workspace,
    document: DocumentId,
    screen: &Screen<'_>,
    state: &mut suggest::State,
) -> (suggest::Pointer, Option<usize>) {
    let target = response
        .hovered()
        .then(|| ui.input(|input| input.pointer.hover_pos()))
        .flatten()
        .and_then(|pos| screen.char_at(ui, pos));
    let pointer = suggest::pointer(ui, workspace, state, target);
    let at = |offset: usize| screen.point_of(offset);
    let anchors = suggest::Anchors {
        clip: screen.layout.text,
        at: &at,
        line_height: LINE_HEIGHT,
    };
    (pointer, suggest::show(ui, workspace, document, &anchors))
}

fn keyboard(
    ui: &Ui,
    workspace: &mut Workspace,
    document: DocumentId,
    page: usize,
    arrived: bool,
    outcome: &mut Outcome,
) {
    // With Tab moving focus (Ctrl+M), Tab and Shift+Tab leave the editor;
    // a Tab that just brought the focus here is not typed either.
    let tab_moves_focus = crate::focus::tab_moves_focus(ui.ctx());
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            id(),
            EventFilter {
                tab: !tab_moves_focus,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            },
        );
    });
    let events = ui.input(|input| input.events.clone());
    let typed = events.iter().filter(|event| {
        !((tab_moves_focus || arrived) && matches!(event, egui::Event::Key { key: Key::Tab, .. }))
    });
    for command in typed.filter_map(|event| keys::command(event, page)) {
        run(workspace, document, command, outcome);
    }
}

fn run(workspace: &mut Workspace, id: DocumentId, command: Command, outcome: &mut Outcome) {
    outcome.changed = true;
    let result = match command {
        Command::Type(text) => workspace.type_text(id, &text),
        Command::DeleteLeft => workspace.delete_left(id),
        Command::DeleteRight => workspace.delete_right(id),
        Command::Enter => workspace.enter(id),
        Command::Tab => workspace.tab(id),
        Command::Move(movement, select) => workspace.move_cursor(id, movement, select),
        Command::Undo => workspace.undo(id).map(drop),
        Command::Redo => workspace.redo(id).map(drop),
        Command::SelectAll => workspace.select_all(id),
        Command::Escape => workspace.cancel_selections(id).map(drop),
        Command::AddNextOccurrence => workspace.add_next_occurrence(id).map(drop),
        Command::AddCursorAbove => workspace.add_cursor_above(id).map(drop),
        Command::AddCursorBelow => workspace.add_cursor_below(id).map(drop),
    };
    if let Err(err) = result {
        outcome.error = Some(err.to_string());
    }
}

/// A press places the caret (Shift+press extends the selection) and a drag
/// selects from where the press was.
fn pointer(
    ui: &Ui,
    response: &Response,
    workspace: &mut Workspace,
    document: DocumentId,
    layout: &Layout,
    view: &mut View,
) {
    let Some(pos) = response.interact_pointer_pos() else {
        view.drag_anchor = None;
        return;
    };
    let Some(doc) = workspace.document(document) else {
        return;
    };
    let offset = offset_at(ui, doc, layout, view, pos);
    let press = ui.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
                ..
            } => Some(modifiers.shift || input.modifiers.shift),
            _ => None,
        })
    });
    let pressed = press.is_some();
    if let Some(shift) = press {
        view.drag_anchor = Some(if shift {
            doc.primary_selection().anchor
        } else {
            offset
        });
    }
    if let Some(anchor) = view.drag_anchor
        && (pressed || response.dragged())
        && let Err(err) =
            workspace.set_selections(document, vec![Selection::new(anchor, offset)], 0)
    {
        tracing::warn!(%err, "cannot place the caret");
    }
}

/// The char offset under a screen position.
fn offset_at(ui: &Ui, doc: &Document, layout: &Layout, view: &View, pos: Pos2) -> usize {
    let below_top = pos.y - layout.origin(view).y;
    let line = if below_top < 0.0 {
        view.top.saturating_sub(1)
    } else {
        view.top + whole_lines(below_top, layout.line_height)
    };
    let line = line.min(doc.line_count() - 1);
    let display = DisplayLine::new(&doc.line(line), layout.tab_size);
    let galley = layout_line(ui, &display, layout);
    let x = pos.x - layout.origin(view).x;
    let index = galley.cursor_from_pos(Vec2::new(x, 0.0)).index.0;
    doc.offset_of(Position::new(line, display.column_at(index)))
}

fn layout_line(ui: &Ui, display: &DisplayLine, layout: &Layout) -> Arc<Galley> {
    let mut text = display.text.clone();
    if display.truncated {
        text.push('…');
    }
    ui.painter()
        .layout_no_wrap(text, layout.font.clone(), FOREGROUND)
}

/// Scrolls by the mouse wheel, three lines a notch; Shift scrolls sideways.
fn wheel(ui: &Ui, view: &mut View, line_count: usize, line_height: f32) {
    let page = ui.available_rect_before_wrap().height();
    let delta = ui.input(|input| {
        input
            .events
            .iter()
            .fold(Vec2::ZERO, |sum, event| match event {
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } if !modifiers.command && !modifiers.ctrl => {
                    let scale = match unit {
                        egui::MouseWheelUnit::Point => 1.0,
                        egui::MouseWheelUnit::Line => 3.0 * line_height,
                        egui::MouseWheelUnit::Page => page,
                    };
                    let delta = *delta * scale;
                    sum + if modifiers.shift && delta.x.abs() < f32::EPSILON {
                        Vec2::new(delta.y, 0.0)
                    } else {
                        delta
                    }
                }
                _ => sum,
            })
    });
    scroll_down(view, -delta.y, line_count, line_height);
    view.left = (view.left - delta.x).max(0.0);
}

/// Moves the view down by `by` points (up if negative). The last line can
/// be scrolled to the top, as with VS Code's `editor.scrollBeyondLastLine`,
/// on by default (`src/vs/editor/common/config/editorOptions.ts:6791-6794`).
fn scroll_down(view: &mut View, by: f32, line_count: usize, line_height: f32) {
    let target = view.offset + by;
    let lines = (target / line_height).floor();
    view.offset = target - lines * line_height;
    if lines >= 0.0 {
        view.top = view.top.saturating_add(count(lines));
    } else if count(-lines) <= view.top {
        view.top -= count(-lines);
    } else {
        view.top = 0;
        view.offset = 0.0;
    }
    let last = line_count.saturating_sub(1);
    if view.top >= last {
        view.top = last;
        view.offset = 0.0;
    }
}

/// Scrolls the least that brings the primary caret into view: a caret above
/// the view goes to the top, one below it to the bottom. (VS Code also keeps
/// context lines around it; not done here.)
fn reveal(ui: &Ui, doc: &Document, layout: &Layout, view: &mut View) {
    let head = doc.position_of(doc.primary_selection().head);
    let height = layout.text.height();
    if head.line < view.top || (head.line == view.top && view.offset > 0.0) {
        view.top = head.line;
        view.offset = 0.0;
    } else if head.line - view.top >= whole_lines(height + view.offset, layout.line_height) {
        let rows = lines_covering(height, layout.line_height).max(1);
        view.top = (head.line + 1).saturating_sub(rows);
        view.offset = (float(rows) * layout.line_height - height).max(0.0);
    }
    let display = DisplayLine::new(&doc.line(head.line), layout.tab_size);
    let galley = layout_line(ui, &display, layout);
    let shown = head.column.min(display.columns());
    let x = galley
        .pos_from_cursor(CCursor::new(display.display_index(shown)))
        .min
        .x;
    let width = layout.text.width();
    if x < view.left {
        view.left = (x - REVEAL_MARGIN).max(0.0);
    } else if x > view.left + width - REVEAL_MARGIN {
        view.left = x - width + REVEAL_MARGIN;
    }
}

/// The color of text of token kind `kind`.
fn token_color(kind: TokenKind) -> Color32 {
    let [_, r, g, b] = kind.dark_modern().to_be_bytes();
    Color32::from_rgb(r, g, b)
}

/// A line laid out in colored sections: `highlights` are the document's,
/// sorted, and `start` is the line's first char.
fn layout_row(
    ui: &Ui,
    display: &DisplayLine,
    layout: &Layout,
    highlights: &[Highlight],
    start: usize,
) -> Arc<Galley> {
    let format = |color| TextFormat::simple(layout.font.clone(), color);
    let mut job = LayoutJob {
        break_on_newline: false,
        ..LayoutJob::default()
    };
    let mut done = 0;
    let first = highlights.partition_point(|h| h.range.end <= start);
    for highlight in &highlights[first..] {
        let from = display.byte_index(highlight.range.start.saturating_sub(start));
        let to = display.byte_index(highlight.range.end - start);
        if from >= display.text.len() {
            break;
        }
        job.append(&display.text[done..from], 0.0, format(FOREGROUND));
        job.append(
            &display.text[from..to],
            0.0,
            format(token_color(highlight.kind)),
        );
        done = to;
    }
    job.append(&display.text[done..], 0.0, format(FOREGROUND));
    if display.truncated {
        job.append("…", 0.0, format(FOREGROUND));
    }
    ui.painter().layout_job(job)
}

/// The lines in view, read from the workspace one by one, with their
/// highlights.
fn rows(ui: &Ui, doc: &Document, layout: &Layout, view: &View) -> Vec<Row> {
    let visible = lines_covering(layout.text.height() + view.offset, layout.line_height);
    let end = (view.top + visible).min(doc.line_count());
    let highlights = doc.highlights(view.top..end);
    (view.top..end)
        .map(|line| {
            let text = doc.line(line);
            let display = DisplayLine::new(&text, layout.tab_size);
            let start = doc.offset_of(Position::new(line, 0));
            let galley = layout_row(ui, &display, layout, &highlights, start);
            Row {
                line,
                top: Row::top_of(layout, view, line),
                text,
                display,
                galley,
            }
        })
        .collect()
}

fn paint(ui: &Ui, screen: &Screen<'_>) {
    let Screen {
        doc,
        layout,
        view,
        rows,
        ..
    } = screen;
    let text_painter = ui.painter_at(layout.text);
    let gutter_painter = ui.painter_at(layout.gutter);
    gutter_painter.rect_filled(layout.gutter, 0.0, BACKGROUND);
    text_painter.rect_filled(layout.text, 0.0, BACKGROUND);
    let carets = selections(&text_painter, screen);
    let left = layout.origin(view).x;
    find_matches(&text_painter, screen);
    for row in rows {
        let y = row.top + (layout.line_height - row.galley.size().y) / 2.0;
        text_painter.galley(Pos2::new(left, y), Arc::clone(&row.galley), FOREGROUND);
    }
    squiggles(&text_painter, screen);
    // VS Code hides the cursors of an editor without focus
    // (`src/vs/editor/browser/viewParts/viewCursors/viewCursors.ts:233-235`).
    if ui.memory(|memory| memory.has_focus(id())) {
        for caret in carets {
            text_painter.rect_filled(caret, 0.0, CARET);
        }
    }
    let active = doc.position_of(doc.primary_selection().head).line;
    for row in rows {
        let color = if row.line == active {
            ACTIVE_LINE_NUMBER
        } else {
            LINE_NUMBER
        };
        let pos = Pos2::new(
            layout.gutter.right() - layout.char_width,
            row.top + layout.line_height / 2.0,
        );
        gutter_painter.text(
            pos,
            Align2::RIGHT_CENTER,
            row.line + 1,
            layout.font.clone(),
            color,
        );
    }
}

/// Paints the squiggles of the diagnostics on the visible lines, the most
/// severe on top.
fn squiggles(painter: &egui::Painter, screen: &Screen<'_>) {
    let Screen {
        doc,
        layout,
        view,
        rows,
        markers,
        ..
    } = screen;
    for marker in markers {
        let from = doc.position_of(marker.range.start);
        let to = doc.position_of(marker.range.end);
        for row in rows
            .iter()
            .filter(|row| (from.line..=to.line).contains(&row.line))
        {
            let first = if row.line == from.line {
                from.column
            } else {
                0
            };
            let last = if row.line == to.line {
                to.column
            } else {
                row.display.columns()
            };
            let x0 = row.x(layout, view, first);
            let x1 = row.x(layout, view, last).max(x0 + layout.char_width);
            crate::problems::paint_squiggle(
                painter,
                marker.severity,
                x0,
                x1,
                row.top + layout.line_height,
            );
        }
    }
}

impl Screen<'_> {
    /// The offsets from the first visible line's start to the line after the
    /// last one.
    fn offsets(&self) -> Range {
        let lines = self
            .rows
            .first()
            .map_or(0..0, |first| first.line..first.line + self.rows.len());
        Range::new(
            self.doc.offset_of(Position::new(lines.start, 0)),
            self.doc.offset_of(Position::new(lines.end, 0)),
        )
    }

    /// The rectangles that cover a non-empty `range` on the visible lines:
    /// one per line, reaching past a line's end when the range does.
    fn rects(&self, range: Range) -> Vec<Rect> {
        let Screen {
            doc, layout, view, ..
        } = self;
        let from = doc.position_of(range.start);
        let to = doc.position_of(range.end);
        self.rows
            .iter()
            .filter(|row| !range.is_empty() && (from.line..=to.line).contains(&row.line))
            .map(|row| {
                let first = if row.line == from.line {
                    from.column
                } else {
                    0
                };
                let x0 = row.x(layout, view, first);
                let x1 = if row.line == to.line {
                    row.x(layout, view, to.column)
                } else {
                    row.x(layout, view, row.display.columns()) + layout.char_width / 2.0
                };
                Rect::from_x_y_ranges(x0..=x1, row.top..=row.top + layout.line_height)
            })
            .collect()
    }
}

/// Paints the selections that touch the visible lines; returns their carets.
fn selections(painter: &egui::Painter, screen: &Screen<'_>) -> Vec<Rect> {
    let Screen {
        doc,
        layout,
        view,
        rows,
        ..
    } = screen;
    let shown = screen.offsets();
    let mut carets = Vec::new();
    for selection in doc.selections() {
        let range = selection.range();
        if range.end < shown.start || range.start > shown.end {
            continue;
        }
        for rect in screen.rects(range) {
            painter.rect_filled(rect, 0.0, SELECTION);
        }
        let head = doc.position_of(selection.head);
        if let Some(row) = rows.iter().find(|row| row.line == head.line) {
            carets.push(caret(layout, view, row, head.column));
        }
    }
    carets
}

/// The caret before `column` of `row`, on whole pixels.
fn caret(layout: &Layout, view: &View, row: &Row, column: usize) -> Rect {
    let x = row.x(layout, view, column);
    let width = caret_width(layout.pixels_per_point);
    let shift = if width >= 2.0 && x - layout.text.left() >= 1.0 {
        1.0
    } else {
        0.0
    };
    let left = (x - shift).round_to_pixels(layout.pixels_per_point);
    Rect::from_min_size(
        Pos2::new(left, row.top),
        Vec2::new(width, layout.line_height),
    )
}

/// Highlights the find matches on the visible lines, the one the primary
/// selection is on as the current match, as VS Code's find decorations do
/// (`src/vs/editor/contrib/find/browser/findWidget.css`, `.findMatch` and
/// `.currentFindMatch`).
fn find_matches(painter: &egui::Painter, screen: &Screen<'_>) {
    let shown = screen.offsets();
    let current = screen.doc.primary_selection().range();
    let first = screen.matches.partition_point(|m| m.end < shown.start);
    let visible = screen.matches[first..]
        .iter()
        .take_while(|m| m.start <= shown.end);
    for &found in visible {
        let color = if found == current {
            FIND_MATCH
        } else {
            FIND_MATCH_HIGHLIGHT
        };
        for rect in screen.rects(found) {
            painter.rect_filled(rect, 0.0, color);
        }
    }
}

/// Exposes the editor to screen readers through AccessKit: a multi-line
/// text input named after the file, whose text runs are the visible lines,
/// with the primary selection as its text selection. Nothing is built while
/// no assistive technology has asked for the tree.
fn expose(ui: &Ui, response: &Response, screen: &Screen<'_>, label: &str) {
    let Screen {
        doc,
        layout,
        view,
        rows,
        ..
    } = screen;
    let ctx = ui.ctx();
    if ctx.accesskit_node_builder(id(), |_| ()).is_none() {
        return;
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, label));
    let text = rows
        .iter()
        .map(|row| row.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let format = TextFormat {
        font_id: layout.font.clone(),
        line_height: Some(layout.line_height),
        color: FOREGROUND,
        ..TextFormat::default()
    };
    let galley = ctx.fonts_mut(|fonts| fonts.layout_job(LayoutJob::single_section(text, format)));
    let origin = layout.origin(view).to_vec2();
    egui::text_selection::accesskit_text::update_accesskit_for_text_widget(
        ctx,
        id(),
        exposed_selection(doc, rows),
        accesskit::Role::MultilineTextInput,
        TSTransform::from_translation(origin),
        &galley,
    );
}

/// The primary selection in the exposed text, if its caret is in view; an
/// anchor out of view is clamped to the exposed text.
fn exposed_selection(doc: &Document, rows: &[Row]) -> Option<CCursorRange> {
    let (first, last) = (rows.first()?, rows.last()?);
    let lengths: Vec<usize> = rows.iter().map(|row| row.text.chars().count()).collect();
    let total = lengths.iter().sum::<usize>() + lengths.len() - 1;
    let index = |position: Position| -> usize {
        if position.line < first.line {
            return 0;
        }
        let before = position.line.min(last.line + 1) - first.line;
        let start: usize = lengths[..before].iter().map(|len| len + 1).sum();
        (start + position.column).min(total)
    };
    let selection = doc.primary_selection();
    let head = doc.position_of(selection.head);
    if !(first.line..=last.line).contains(&head.line) {
        return None;
    }
    let anchor = index(doc.position_of(selection.anchor));
    Some(CCursorRange::two(
        CCursor::new(anchor),
        CCursor::new(index(head)),
    ))
}
