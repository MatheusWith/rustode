//! The tab strip above the editor: one tab per open document, in the
//! workspace's order, with the active one highlighted, a dot on tabs with
//! unsaved changes and a close button. In the AccessKit tree it is a tab
//! list whose tabs carry the file name and the selected state; a tab with
//! unsaved changes says so in its description (rustode's addition: VS
//! Code's tab label is the name alone).
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//!
//! - 35-point tabs (`src/vs/workbench/browser/parts/editor/editorTabsControl.ts:108-117`,
//!   `:664`; `src/vs/base/common/font.ts:59`) with 13-point labels (`:54`);
//! - `workbench.editor.tabSizing` `fit`, the default
//!   (`src/vs/workbench/browser/workbench.contribution.ts:231-234`): a tab is
//!   at least 120 points wide and wide enough for its label, with 10 points
//!   of padding on the left and a 28-point actions slot on the right
//!   (`src/vs/workbench/browser/parts/editor/media/multieditortabscontrol.css:115-124`,
//!   `:211-215`, `:448-452`);
//! - the close button shows a filled circle while the file has unsaved
//!   changes (`:507-513`) and is labelled "Close"
//!   (`src/vs/workbench/browser/parts/editor/editorActions.ts:447-450`);
//! - a tab list of tabs named after the file, the active one selected
//!   (`src/vs/workbench/browser/parts/editor/multiEditorTabsControl.ts:276-280`,
//!   `:1908`; `src/vs/workbench/common/editor/editorInput.ts:178-180`);
//! - Dark Modern's tab colors
//!   (`extensions/theme-defaults/themes/dark_modern.json:37-39`, `:104-112`).
//!
//! The strip scrolls sideways just enough to keep the active tab in view.

use eframe::egui::{
    Color32, FontId, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, UiBuilder, Vec2, accesskit, pos2,
};
use rustode_workspace::{DocumentId, Workspace};

/// Height of the tab strip, in points.
pub const TAB_HEIGHT: f32 = 35.0;

const LABEL_SIZE: f32 = 13.0;
const PADDING: f32 = 10.0;
/// Width of the slot on the right that holds the close button.
const ACTIONS: f32 = 28.0;
const MIN_WIDTH: f32 = 120.0;

const STRIP: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const ACTIVE_BACKGROUND: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
const ACTIVE_FOREGROUND: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const INACTIVE_FOREGROUND: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);
const ACTIVE_BORDER_TOP: Color32 = Color32::from_rgb(0x00, 0x78, 0xd4);

/// What a tab says to screen readers when its file has unsaved changes.
pub(crate) const UNSAVED: &str = "Unsaved changes";

/// Where the strip is scrolled to, kept between frames.
#[derive(Debug, Default)]
pub(crate) struct Strip {
    scroll: f32,
}

/// What a click on the strip asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Activate(DocumentId),
    Close(DocumentId),
}

/// One tab, measured.
struct Tab {
    id: DocumentId,
    name: String,
    dirty: bool,
    active: bool,
    width: f32,
}

/// Draws the strip over the whole of `ui` and returns what a click asked.
pub(crate) fn show(ui: &mut Ui, workspace: &Workspace, strip: &mut Strip) -> Option<Action> {
    let rect = ui.max_rect();
    // Tabs scrolled out of the strip neither draw nor take clicks.
    ui.shrink_clip_rect(rect);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, STRIP);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, BORDER),
    );
    let tabs = measure(ui, workspace);
    strip.reveal(&tabs, rect.width());
    ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
        node.set_role(accesskit::Role::TabList);
        node.set_bounds(crate::a11y::bounds(rect));
    });
    let mut action = None;
    let mut x = rect.left() - strip.scroll;
    for tab in &tabs {
        let place = Rect::from_min_size(pos2(x, rect.top()), Vec2::new(tab.width, TAB_HEIGHT));
        x += tab.width;
        if let Some(clicked) = show_tab(ui, tab, place) {
            action = Some(clicked);
        }
    }
    action
}

fn font() -> FontId {
    FontId::proportional(LABEL_SIZE)
}

fn measure(ui: &Ui, workspace: &Workspace) -> Vec<Tab> {
    let active = workspace.active_document();
    workspace
        .tabs()
        .iter()
        .filter_map(|&id| workspace.document(id).map(|doc| (id, doc)))
        .map(|(id, doc)| {
            let name = doc
                .path()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let label = ui
                .painter()
                .layout_no_wrap(name.clone(), font(), ACTIVE_FOREGROUND)
                .size()
                .x;
            Tab {
                id,
                name,
                dirty: doc.is_dirty(),
                active: Some(id) == active,
                width: (PADDING + label + ACTIONS).ceil().max(MIN_WIDTH),
            }
        })
        .collect()
}

impl Strip {
    /// Scrolls the least that shows the whole active tab.
    fn reveal(&mut self, tabs: &[Tab], width: f32) {
        let total: f32 = tabs.iter().map(|tab| tab.width).sum();
        let mut start = 0.0;
        for tab in tabs {
            if tab.active {
                let end = start + tab.width;
                if start < self.scroll {
                    self.scroll = start;
                } else if end > self.scroll + width {
                    self.scroll = end - width;
                }
                break;
            }
            start += tab.width;
        }
        self.scroll = self.scroll.min((total - width).max(0.0)).max(0.0);
    }
}

fn tab_id(id: DocumentId) -> Id {
    Id::new(("rustode-tab", id))
}

/// Draws one tab and its close button, exposes them, and returns a click.
fn show_tab(ui: &mut Ui, tab: &Tab, place: Rect) -> Option<Action> {
    let id = tab_id(tab.id);
    let response = ui.interact(place, id, Sense::CLICK);
    // The tab's node first: the close button's node is its child.
    expose(ui, id, place, |node| {
        node.set_role(accesskit::Role::Tab);
        node.set_label(tab.name.clone());
        node.set_selected(tab.active);
        if tab.dirty {
            node.set_description(UNSAVED);
        }
    });
    let slot = Rect::from_min_size(
        pos2(place.right() - ACTIONS, place.top()),
        Vec2::new(ACTIONS, TAB_HEIGHT),
    );
    let close_id = Id::new(("rustode-tab-close", tab.id));
    let close = ui
        .scope_builder(
            UiBuilder::new()
                .max_rect(slot)
                .id_salt(close_id)
                .accessibility_parent(id),
            |ui| {
                let close = ui.interact(slot, close_id, Sense::CLICK);
                expose(ui, close_id, slot, |node| {
                    node.set_role(accesskit::Role::Button);
                    node.set_label("Close");
                });
                close
            },
        )
        .inner;
    paint(
        ui,
        tab,
        place,
        response.hovered() || close.hovered(),
        close.hovered(),
    );
    if close.clicked() {
        Some(Action::Close(tab.id))
    } else if response.clicked() {
        Some(Action::Activate(tab.id))
    } else {
        None
    }
}

/// Fills the AccessKit node `id` with its bounds, a click action and `fill`.
fn expose(ui: &Ui, id: Id, rect: Rect, fill: impl FnOnce(&mut accesskit::Node)) {
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_bounds(crate::a11y::bounds(rect));
        node.add_action(accesskit::Action::Click);
        fill(node);
    });
}

fn paint(ui: &Ui, tab: &Tab, place: Rect, hovered: bool, close_hovered: bool) {
    let painter = ui.painter_at(ui.clip_rect().intersect(place));
    let (background, foreground) = if tab.active {
        (ACTIVE_BACKGROUND, ACTIVE_FOREGROUND)
    } else {
        (STRIP, INACTIVE_FOREGROUND)
    };
    painter.rect_filled(place, 0.0, background);
    if tab.active {
        painter.hline(
            place.x_range(),
            place.top() + 0.5,
            Stroke::new(1.0, ACTIVE_BORDER_TOP),
        );
    } else {
        painter.hline(
            place.x_range(),
            place.bottom() - 0.5,
            Stroke::new(1.0, BORDER),
        );
    }
    painter.vline(
        place.right() - 0.5,
        place.y_range(),
        Stroke::new(1.0, BORDER),
    );
    painter.text(
        pos2(place.left() + PADDING, place.center().y),
        eframe::egui::Align2::LEFT_CENTER,
        &tab.name,
        font(),
        foreground,
    );
    let center = pos2(place.right() - ACTIONS / 2.0, place.center().y);
    if tab.dirty && !close_hovered {
        painter.circle_filled(center, 4.0, foreground);
    } else if tab.active || hovered {
        cross(&painter, center, foreground);
    }
}

/// The close glyph: an × about 8 points wide.
fn cross(painter: &eframe::egui::Painter, center: Pos2, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let d = Vec2::splat(4.0);
    let e = Vec2::new(4.0, -4.0);
    painter.add(Shape::line_segment([center - d, center + d], stroke));
    painter.add(Shape::line_segment([center - e, center + e], stroke));
}
