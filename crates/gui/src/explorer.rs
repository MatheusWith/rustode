//! The explorer in the sidebar: the "EXPLORER" title, the opened folder's
//! header and its tree, drawn from `Workspace::explorer`. Only the rows in
//! view are drawn and exposed. In the AccessKit tree it is a tree named
//! "Files Explorer" whose items carry the entry name, the level, whether a
//! folder is expanded and which row is selected.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//!
//! - the sidebar title is 35 points high, its label 11 points, upper case,
//!   20 points in (`src/vs/workbench/browser/media/part.css:48-78`,
//!   `src/vs/workbench/browser/parts/sidebar/media/sidebarpart.css:27-30`);
//! - the folder's section header is 22 points high, 11 points, upper case
//!   (`src/vs/base/browser/ui/splitview/paneview.css:10`, `:25-43`), and
//!   without a folder it reads "No Folder Opened" over "You have not yet
//!   opened a folder." (`src/vs/workbench/contrib/files/browser/views/emptyView.ts:29`,
//!   `src/vs/workbench/contrib/files/browser/explorerViewlet.ts:346`);
//! - rows are 22 points high (`src/vs/workbench/contrib/files/browser/views/explorerViewer.ts:78-84`)
//!   in 13-point text (`src/vs/base/common/font.ts:29`), indented 8 points
//!   plus 8 per level (`workbench.tree.indent`,
//!   `src/vs/platform/list/browser/listService.ts:1417-1423`;
//!   `src/vs/base/browser/ui/tree/abstractTree.ts:349`, `:444`), after a
//!   16-point twistie with 6 points of padding
//!   (`src/vs/base/browser/ui/tree/media/tree.css:45-55`);
//! - the tree is "Files Explorer", an item is named after its entry and
//!   its level counts from 1 at the top
//!   (`src/vs/workbench/contrib/files/browser/views/explorerViewer.ts:867-868`, `:1121-1139`);
//! - Dark Modern's sidebar colors
//!   (`extensions/theme-defaults/themes/dark_modern.json:83-89`) and the
//!   color registry's list selection colors and focus outline
//!   (`src/vs/platform/theme/common/colors/listColors.ts:25-27`, `:33-39`,
//!   `:45-47`; `focusBorder`, `dark_modern.json:48`).

use crate::keys::{self, ExplorerKey};
use eframe::egui::{
    self, Align, Color32, EventFilter, FontId, Id, Label, Layout, Pos2, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, Ui, UiBuilder, Vec2, accesskit, pos2,
};
use rustode_workspace::{ExplorerRow, Workspace};
use std::path::Path;

/// Height of the sidebar's title.
pub const TITLE_HEIGHT: f32 = 35.0;
/// Height of the folder's section header.
pub const HEADER_HEIGHT: f32 = 22.0;
/// Height of one row of the tree.
pub const ROW_HEIGHT: f32 = 22.0;
const INDENT: f32 = 8.0;
/// The twistie and the padding after it.
const TWISTIE: f32 = 22.0;
const FONT_SIZE: f32 = 13.0;
const TITLE_SIZE: f32 = 11.0;
/// Rows one wheel notch scrolls.
const WHEEL_ROWS: f32 = 3.0;

const BACKGROUND: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const SELECTION: Color32 = Color32::from_rgb(0x04, 0x39, 0x5e);
const SELECTION_FOREGROUND: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const INACTIVE_SELECTION: Color32 = Color32::from_rgb(0x37, 0x37, 0x3d);
const FOCUS_OUTLINE: Color32 = Color32::from_rgb(0x00, 0x78, 0xd4);

/// The tree's widget id: it takes keyboard focus.
pub(crate) fn id() -> Id {
    Id::new("rustode-explorer")
}

fn row_id(row: &ExplorerRow) -> Id {
    Id::new(("rustode-explorer-row", &row.path))
}

/// What the explorer keeps between frames.
#[derive(Debug, Default)]
pub(crate) struct View {
    /// The selected row.
    selected: usize,
    /// Points scrolled down.
    scroll: f32,
    /// The selected row when the tree last scrolled to it; the tree only
    /// follows the selection again once it moves, so wheel scrolling sticks.
    revealed: Option<usize>,
}

impl View {
    /// Selects the row of `path`, if it is shown.
    pub(crate) fn select(&mut self, workspace: &Workspace, path: &Path) {
        if let Some(index) = rows(workspace).iter().position(|row| row.path == path) {
            self.selected = index;
        }
    }

    /// Keeps the selection on a row and, when it moved, in view.
    fn settle(&mut self, count: usize, height: f32) {
        self.selected = self.selected.min(count.saturating_sub(1));
        if self.revealed != Some(self.selected) {
            let top = float(self.selected) * ROW_HEIGHT;
            if top < self.scroll {
                self.scroll = top;
            } else if top + ROW_HEIGHT > self.scroll + height {
                self.scroll = top + ROW_HEIGHT - height;
            }
            self.revealed = Some(self.selected);
        }
        let max = (float(count) * ROW_HEIGHT - height).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }
}

/// What a frame's input did in the explorer.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// Whether a key or click changed something.
    pub(crate) changed: bool,
    /// Whether focus should go to the editor (Enter on a file, Escape).
    pub(crate) focus_editor: bool,
    /// Why opening an entry failed, for the status bar.
    pub(crate) message: Option<String>,
}

/// A count of rows in view (a few thousand at most) as a float.
#[allow(clippy::cast_precision_loss)] // exact below 2^24
fn float(count: usize) -> f32 {
    count as f32
}

/// A whole, non-negative float as a count.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // float to int casts saturate
fn count(value: f32) -> usize {
    value.max(0.0) as usize
}

fn rows(workspace: &Workspace) -> Vec<ExplorerRow> {
    workspace
        .explorer()
        .map_or_else(Default::default, rustode_workspace::Explorer::rows)
}

/// Draws the sidebar over the whole of `ui`, running what its input asks.
pub(crate) fn show(ui: &mut Ui, workspace: &mut Workspace, view: &mut View) -> Outcome {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, BACKGROUND);
    ui.painter()
        .vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0, BORDER));
    let title = Rect::from_min_size(rect.min, Vec2::new(rect.width(), TITLE_HEIGHT));
    text(ui, title, "EXPLORER", TITLE_SIZE, 20.0);
    let header = Rect::from_min_size(
        pos2(rect.left(), title.bottom()),
        Vec2::new(rect.width(), HEADER_HEIGHT),
    );
    let Some(root) = workspace
        .explorer()
        .map(|explorer| explorer.root().to_owned())
    else {
        text(ui, header, "NO FOLDER OPENED", TITLE_SIZE, 20.0);
        let below = Rect::from_min_size(
            pos2(rect.left() + 20.0, header.bottom() + 4.0),
            Vec2::new(rect.width() - 28.0, 3.0 * ROW_HEIGHT),
        );
        let welcome = RichText::new("You have not yet opened a folder.")
            .size(FONT_SIZE)
            .color(FOREGROUND);
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(below)
                .layout(Layout::top_down(Align::Min)),
            |ui| ui.add(Label::new(welcome).selectable(false).wrap()),
        );
        return Outcome::default();
    };
    chevron(
        ui.painter(),
        pos2(rect.left() + 10.0, header.center().y),
        true,
    );
    let name = root.file_name().map_or_else(Default::default, |name| {
        name.to_string_lossy().to_uppercase()
    });
    text(ui, header, &name, TITLE_SIZE, 20.0);
    let list = Rect::from_min_max(
        pos2(rect.left(), header.bottom()),
        pos2(rect.right() - 1.0, rect.bottom()),
    );
    tree(ui, workspace, view, list)
}

/// The tree in `list`: input, then drawing and exposing the rows in view.
fn tree(ui: &mut Ui, workspace: &mut Workspace, view: &mut View, list: Rect) -> Outcome {
    let response = ui.interact(list, id(), Sense::click());
    let mut outcome = Outcome::default();
    let page = count((list.height() / ROW_HEIGHT).floor()).max(1);
    if response.has_focus() {
        keyboard(ui, workspace, view, page, &mut outcome);
    }
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        click(workspace, view, list, pos, &mut outcome);
        response.request_focus();
    }
    if response.contains_pointer() {
        wheel(ui, view, list.height());
    }
    let rows = rows(workspace);
    view.settle(rows.len(), list.height());
    let first = count((view.scroll / ROW_HEIGHT).floor());
    let shown = count((list.height() / ROW_HEIGHT).ceil()) + 1;
    let visible: Vec<(usize, &ExplorerRow, Rect)> = rows
        .iter()
        .enumerate()
        .skip(first)
        .take(shown)
        .map(|(index, row)| {
            let top = list.top() - view.scroll + float(index) * ROW_HEIGHT;
            let place =
                Rect::from_min_size(pos2(list.left(), top), Vec2::new(list.width(), ROW_HEIGHT));
            (index, row, place)
        })
        .filter(|(_, _, place)| place.intersects(list))
        .collect();
    paint(ui, list, &visible, view.selected, response.has_focus());
    expose(ui, list, &visible, view.selected);
    outcome
}

fn keyboard(
    ui: &Ui,
    workspace: &mut Workspace,
    view: &mut View,
    page: usize,
    outcome: &mut Outcome,
) {
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            id(),
            EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            },
        );
    });
    // The explorer takes this frame's keys, so a key that moves focus to
    // the editor (Enter) does not reach it too.
    let events = ui.ctx().input_mut(|input| {
        let events = input.events.clone();
        input
            .events
            .retain(|event| !matches!(event, egui::Event::Key { .. } | egui::Event::Text(_)));
        events
    });
    for key in events.iter().filter_map(keys::explorer_key) {
        outcome.changed = true;
        apply(workspace, view, key, page, outcome);
    }
}

/// One explorer key, as the TUI's explorer handles it.
fn apply(
    workspace: &mut Workspace,
    view: &mut View,
    key: ExplorerKey,
    page: usize,
    outcome: &mut Outcome,
) {
    let rows = rows(workspace);
    let last = rows.len().saturating_sub(1);
    let row = view.selected.min(last);
    match key {
        ExplorerKey::Up => view.selected = row.saturating_sub(1),
        ExplorerKey::Down => view.selected = (row + 1).min(last),
        ExplorerKey::PageUp => view.selected = row.saturating_sub(page),
        ExplorerKey::PageDown => view.selected = (row + page).min(last),
        ExplorerKey::Home => view.selected = 0,
        ExplorerKey::End => view.selected = last,
        ExplorerKey::Leave => outcome.focus_editor = true,
        ExplorerKey::Open { keep_focus } => {
            if let Some(entry) = rows.get(row) {
                open(workspace, entry, keep_focus, outcome);
            }
        }
        ExplorerKey::Collapse => collapse(workspace, view, &rows, row, outcome),
        ExplorerKey::Expand => expand(workspace, view, &rows, row, outcome),
    }
}

/// A folder toggles; a file opens in a tab, and unless `keep_focus` the
/// focus goes to the editor.
fn open(workspace: &mut Workspace, entry: &ExplorerRow, keep_focus: bool, outcome: &mut Outcome) {
    if entry.is_dir {
        toggle(workspace, &entry.path, outcome);
        return;
    }
    match workspace.open(&entry.path) {
        Ok(_) => outcome.focus_editor |= !keep_focus,
        Err(err) => outcome.message = Some(err.to_string()),
    }
}

/// Left: collapse an expanded folder, else select the parent.
fn collapse(
    workspace: &mut Workspace,
    view: &mut View,
    rows: &[ExplorerRow],
    row: usize,
    outcome: &mut Outcome,
) {
    let Some(entry) = rows.get(row) else { return };
    if entry.is_dir && entry.expanded {
        toggle(workspace, &entry.path, outcome);
    } else if let Some(parent) = entry.path.parent()
        && let Some(index) = rows.iter().position(|r| r.path == parent)
    {
        view.selected = index;
    }
}

/// Right: expand a collapsed folder, else go to its first child.
fn expand(
    workspace: &mut Workspace,
    view: &mut View,
    rows: &[ExplorerRow],
    row: usize,
    outcome: &mut Outcome,
) {
    let Some(entry) = rows.get(row).filter(|entry| entry.is_dir) else {
        return;
    };
    if !entry.expanded {
        toggle(workspace, &entry.path, outcome);
    } else if rows
        .get(row + 1)
        .is_some_and(|next| next.depth > entry.depth)
    {
        view.selected = row + 1;
    }
}

fn toggle(workspace: &mut Workspace, path: &Path, outcome: &mut Outcome) {
    if let Err(err) = workspace.toggle_folder(path) {
        outcome.message = Some(err.to_string());
    }
}

/// A click selects the row under the pointer and opens it, keeping focus.
fn click(workspace: &mut Workspace, view: &mut View, list: Rect, pos: Pos2, outcome: &mut Outcome) {
    let index = count(((pos.y - list.top() + view.scroll) / ROW_HEIGHT).floor());
    let rows = rows(workspace);
    if let Some(entry) = rows.get(index) {
        outcome.changed = true;
        view.selected = index;
        open(workspace, entry, true, outcome);
    }
}

/// Scrolls by the mouse wheel, three rows a notch.
fn wheel(ui: &Ui, view: &mut View, page: f32) {
    let delta = ui.input(|input| {
        input.events.iter().fold(0.0, |sum, event| match event {
            egui::Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } if !modifiers.command && !modifiers.ctrl => {
                let scale = match unit {
                    egui::MouseWheelUnit::Point => 1.0,
                    egui::MouseWheelUnit::Line => WHEEL_ROWS * ROW_HEIGHT,
                    egui::MouseWheelUnit::Page => page,
                };
                sum + delta.y * scale
            }
            _ => sum,
        })
    });
    view.scroll -= delta;
}

fn paint(
    ui: &Ui,
    list: Rect,
    visible: &[(usize, &ExplorerRow, Rect)],
    selected: usize,
    focused: bool,
) {
    let painter = ui.painter_at(list);
    let font = FontId::proportional(FONT_SIZE);
    for &(index, row, place) in visible {
        let mut color = FOREGROUND;
        if index == selected {
            if focused {
                painter.rect_filled(place, 0.0, SELECTION);
                painter.rect_stroke(
                    place,
                    0.0,
                    Stroke::new(1.0, FOCUS_OUTLINE),
                    StrokeKind::Inside,
                );
                color = SELECTION_FOREGROUND;
            } else {
                painter.rect_filled(place, 0.0, INACTIVE_SELECTION);
            }
        }
        let indent = list.left() + INDENT + float(row.depth) * INDENT;
        if row.is_dir {
            chevron(&painter, pos2(indent + 8.0, place.center().y), row.expanded);
        }
        painter.text(
            pos2(indent + TWISTIE, place.center().y),
            egui::Align2::LEFT_CENTER,
            &row.name,
            font.clone(),
            color,
        );
    }
}

/// VS Code's twistie: a chevron pointing right, or down when expanded.
pub(crate) fn chevron(painter: &egui::Painter, center: Pos2, expanded: bool) {
    let points = if expanded {
        vec![
            center + Vec2::new(-4.0, -2.0),
            center + Vec2::new(0.0, 2.0),
            center + Vec2::new(4.0, -2.0),
        ]
    } else {
        vec![
            center + Vec2::new(-2.0, -4.0),
            center + Vec2::new(2.0, 0.0),
            center + Vec2::new(-2.0, 4.0),
        ]
    };
    painter.add(Shape::line(points, Stroke::new(1.2, FOREGROUND)));
}

/// The tree and its rows in view, for screen readers.
fn expose(ui: &mut Ui, list: Rect, visible: &[(usize, &ExplorerRow, Rect)], selected: usize) {
    let ctx = ui.ctx().clone();
    let exposed = ctx.accesskit_node_builder(id(), |node| {
        node.set_role(accesskit::Role::Tree);
        node.set_label("Files Explorer");
    });
    if exposed.is_none() {
        return;
    }
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(list)
            .id_salt("rustode-explorer-rows")
            .accessibility_parent(id()),
        |ui| {
            for &(index, row, place) in visible {
                let row_id = row_id(row);
                ui.interact(place.intersect(list), row_id, Sense::hover());
                ctx.accesskit_node_builder(row_id, |node| {
                    node.set_role(accesskit::Role::TreeItem);
                    node.set_label(row.name.clone());
                    node.set_level(row.depth + 1);
                    if row.is_dir {
                        node.set_expanded(row.expanded);
                    }
                    node.set_selected(index == selected);
                    node.set_bounds(crate::a11y::bounds(place));
                });
                if index == selected {
                    ctx.accesskit_node_builder(id(), |node| {
                        node.set_active_descendant(row_id.accesskit_id());
                    });
                }
            }
        },
    );
}

/// A line of sidebar text in `rect`, `left` points in, as a label.
fn text(ui: &mut Ui, rect: Rect, text: &str, size: f32, left: f32) {
    let place = Rect::from_min_max(
        pos2(rect.left() + left, rect.top()),
        pos2(rect.right() - 8.0, rect.bottom()),
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(place)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.add(
                Label::new(RichText::new(text).size(size).color(FOREGROUND))
                    .selectable(false)
                    .truncate(),
            );
        },
    );
}
