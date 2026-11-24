//! The comparison of a disk conflict, side by side.
//!
//! The workspace computes the diff
//! ([`Workspace::conflict_compare`](rustode_workspace::Workspace::conflict_compare);
//! behavior studied in VS Code and reimplemented, see
//! `crates/workspace/src/compare.rs` and `ATTRIBUTION.md`); this view only
//! draws it, read-only: the file on disk on the left, the unsaved text on
//! the right, a line only in one of them tinted red (file) or green (text)
//! and marked `-` or `+`, so the meaning does not rest on color. Only the
//! rows in view are laid out, so a long file costs the same as a short one.
//!
//! The buttons run registered commands, the ones the prompt runs: Overwrite
//! is VS Code's "Use your changes and overwrite file contents" and Revert
//! its "Discard your changes and revert to file contents"; Close keeps the
//! conflict. Keys: Up, Down, `PageUp`, `PageDown`, Home and End scroll, `N` and
//! `P` go to the next and previous change, Esc closes.

use eframe::egui::{
    self, Align, Color32, Context, FontId, Frame, Id, Key, Label, Layout, Modifiers, Order, Rect,
    RichText, ScrollArea, Sense, Stroke, UiBuilder, Vec2,
};
use rustode_workspace::{ConflictCompare, SideBySide, SideCell};

const BACKGROUND: Color32 = Color32::from_rgb(0x1F, 0x1F, 0x1F);
const BORDER: Color32 = Color32::from_rgb(0x45, 0x45, 0x45);
const TEXT: Color32 = Color32::from_rgb(0xCC, 0xCC, 0xCC);
const DIM: Color32 = Color32::from_rgb(0x9D, 0x9D, 0x9D);
/// Behind a line only in the file (VS Code's removed-line red).
const REMOVED: Color32 = Color32::from_rgba_premultiplied(40, 0, 0, 40);
/// Behind a line only in the unsaved text (VS Code's inserted-line green).
const ADDED: Color32 = Color32::from_rgba_premultiplied(30, 36, 17, 50);
const ROW_HEIGHT: f32 = 18.0;
const FONT_SIZE: f32 = 13.0;
const MARGIN: f32 = 16.0;

/// What the user did with the comparison this frame.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    /// Nothing yet.
    None,
    /// Run this command of the registry, and close the comparison.
    Run(&'static str),
    /// Close the comparison and keep editing; the conflict stays.
    Close,
}

/// The open comparison.
#[derive(Debug)]
pub(crate) struct State {
    model: ConflictCompare,
    sides: SideBySide,
    /// The change last jumped to, an index into `sides.changes`.
    current: usize,
    /// The row to scroll to at the next frame.
    scroll_to: Option<usize>,
    /// The first row in view, and how many rows fit, as of the last frame.
    top: usize,
    page: usize,
    /// Whether the view took the keyboard focus from the editor yet.
    focused: bool,
}

impl State {
    pub(crate) fn new(model: ConflictCompare) -> Self {
        let sides = model.side_by_side();
        let scroll_to = sides.changes.first().map(|row| row.saturating_sub(2));
        Self {
            model,
            sides,
            current: 0,
            scroll_to,
            top: 0,
            page: 10,
            focused: false,
        }
    }

    /// The document in conflict.
    pub(crate) fn document(&self) -> rustode_workspace::DocumentId {
        self.model.document
    }

    fn go(&mut self, forward: bool) {
        let count = self.sides.changes.len();
        let target = if forward {
            Some(self.current + 1).filter(|n| *n < count)
        } else {
            self.current.checked_sub(1)
        };
        if let Some(index) = target {
            self.current = index;
            self.scroll_to = Some(self.sides.changes[index].saturating_sub(1));
        }
    }

    fn scroll_by(&mut self, rows: isize) {
        let top = self.scroll_to.unwrap_or(self.top);
        self.scroll_to = Some(top.saturating_add_signed(rows));
    }

    fn keys(&mut self, ctx: &Context) -> Reply {
        let pressed = |key: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        let page = isize::try_from(self.page).unwrap_or(10);
        if pressed(Key::Escape) {
            return Reply::Close;
        }
        if pressed(Key::ArrowDown) {
            self.scroll_by(1);
        }
        if pressed(Key::ArrowUp) {
            self.scroll_by(-1);
        }
        if pressed(Key::PageDown) {
            self.scroll_by(page);
        }
        if pressed(Key::PageUp) {
            self.scroll_by(-page);
        }
        if pressed(Key::Home) {
            self.scroll_to = Some(0);
        }
        if pressed(Key::End) {
            self.scroll_to = Some(self.sides.rows.len());
        }
        if pressed(Key::N) {
            self.go(true);
        }
        if pressed(Key::P) {
            self.go(false);
        }
        Reply::None
    }
}

/// Draws the comparison over the window and returns what the user did.
pub(crate) fn show(ctx: &Context, state: &mut State) -> Reply {
    let mut reply = state.keys(ctx);
    let mut clicked = Reply::None;
    let screen = ctx.content_rect();
    let frame_rect = screen.shrink(MARGIN);
    egui::Area::new(Id::new("rustode-conflict-compare"))
        .order(Order::Foreground)
        .fixed_pos(frame_rect.min)
        .show(ctx, |ui| {
            Frame::new()
                .fill(BACKGROUND)
                .stroke(Stroke::new(1.0, BORDER))
                .inner_margin(8.0)
                .show(ui, |ui| {
                    // The editor must not take the keys that move through the
                    // comparison, so the view holds the focus while it is open.
                    let holder = ui.interact(
                        frame_rect,
                        Id::new("rustode-compare-focus"),
                        Sense::focusable_noninteractive(),
                    );
                    if !state.focused {
                        holder.request_focus();
                        state.focused = true;
                    }
                    ui.set_min_size(frame_rect.size() - Vec2::splat(18.0));
                    ui.set_max_size(frame_rect.size() - Vec2::splat(18.0));
                    clicked = header(ui, state);
                    body(ui, state);
                });
        });
    if reply == Reply::None {
        reply = clicked;
    }
    reply
}

fn header(ui: &mut egui::Ui, state: &mut State) -> Reply {
    let mut reply = Reply::None;
    let model = &state.model;
    let change = if state.sides.changes.is_empty() {
        String::new()
    } else {
        format!(
            "  change {}/{}",
            state.current + 1,
            state.sides.changes.len()
        )
    };
    let line = format!(
        "{}  +{} -{}{change}",
        model.title, model.added, model.removed
    );
    ui.label(RichText::new(line).strong().color(TEXT));
    ui.horizontal(|ui| {
        if ui.button("Previous change").clicked() {
            state.go(false);
        }
        if ui.button("Next change").clicked() {
            state.go(true);
        }
        ui.separator();
        if ui.button("Overwrite").clicked() {
            reply = Reply::Run("workbench.files.action.saveIgnoreModifiedSince");
        }
        if ui.button("Revert").clicked() {
            reply = Reply::Run("workbench.files.action.revertLocalChanges");
        }
        if ui.button("Close comparison").clicked() {
            reply = Reply::Close;
        }
    });
    if let Some(notice) = &state.model.notice {
        ui.label(RichText::new(notice.as_str()).color(DIM));
    }
    reply
}

fn body(ui: &mut egui::Ui, state: &mut State) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut area = ScrollArea::vertical().auto_shrink([false, false]);
    if let Some(row) = state.scroll_to.take() {
        // `show_rows` clamps the offset to the content.
        area = area.vertical_scroll_offset(crate::editor::float(row) * ROW_HEIGHT);
    }
    let rows = &state.sides.rows;
    let output = area.show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
        for row in &rows[range] {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::hover());
            let half = (rect.width() / 2.0).floor();
            let left = Rect::from_min_size(rect.min, Vec2::new(half - 1.0, ROW_HEIGHT));
            let right = Rect::from_min_size(
                rect.min + Vec2::new(half, 0.0),
                Vec2::new(rect.width() - half, ROW_HEIGHT),
            );
            cell(ui, left, row.disk.as_ref(), row.changed, '-', REMOVED);
            cell(ui, right, row.mine.as_ref(), row.changed, '+', ADDED);
        }
    });
    state.top = crate::editor::count((output.state.offset.y / ROW_HEIGHT).floor());
    state.page = crate::editor::count(output.inner_rect.height() / ROW_HEIGHT).max(1);
}

/// One side of a row: tinted when it is part of a change, with the line's
/// mark and number before its text.
fn cell(
    ui: &mut egui::Ui,
    rect: Rect,
    cell: Option<&SideCell>,
    changed: bool,
    mark: char,
    tint: Color32,
) {
    if changed && cell.is_some() {
        ui.painter().rect_filled(rect, 0.0, tint);
    }
    let Some(cell) = cell else { return };
    let mark = if changed { mark } else { ' ' };
    let text = format!("{mark} {:>5}  {}", cell.number, cell.text);
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.add(
                Label::new(
                    RichText::new(text)
                        .font(FontId::monospace(FONT_SIZE))
                        .color(TEXT),
                )
                .truncate()
                .selectable(false),
            );
        },
    );
}
