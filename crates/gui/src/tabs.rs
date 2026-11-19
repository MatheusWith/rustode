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
//!
//! **Keyboard**, as in VS Code: only the active tab and its close button
//! are Tab stops; on the active tab, Left/Up and Right/Down activate the
//! previous and the next tab and Home and End the first and the last,
//! keeping the focus on the strip, while Enter or Space moves it to the
//! editor (`src/vs/workbench/browser/parts/editor/multiEditorTabsControl.ts:1223-1257`,
//! `:1909-1910`). A focused tab or close button shows the focus outline.

use crate::focus;
use eframe::egui::{
    Color32, EventFilter, FontId, Id, Key, Pos2, Rect, Sense, Shape, Stroke, Ui, UiBuilder, Vec2,
    accesskit, pos2,
};
use rustode_workspace::{DocumentId, EditorGroup, GroupId, Workspace};

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
/// `tab.unfocusedActiveBorderTop` (`dark_modern.json:114`).
const UNFOCUSED_BORDER_TOP: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
/// `tab.unfocusedActiveForeground`: the active foreground at half opacity,
/// blended over the active background.
const UNFOCUSED_FOREGROUND: Color32 = Color32::from_rgb(0x8f, 0x8f, 0x8f);

/// What a tab says to screen readers when its file has unsaved changes.
pub(crate) const UNSAVED: &str = "Unsaved changes";

/// Where the strip is scrolled to, kept between frames.
#[derive(Debug, Default)]
pub(crate) struct Strip {
    scroll: f32,
}

/// What a click or a key on the strip asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    /// Activate the tab and focus the editor.
    Activate(DocumentId),
    /// Activate the tab and keep the focus on the strip.
    Select(DocumentId),
    Close(DocumentId),
}

/// One tab, measured.
struct Tab {
    id: DocumentId,
    name: String,
    dirty: bool,
    active: bool,
    /// Whether the tab's group lacks the focus: its active tab is dimmed.
    unfocused: bool,
    width: f32,
}

/// Draws the strip of `group` over the whole of `ui` and returns what a
/// click asked. The active group's strip is the interactive one (it takes
/// focus and the keys); another group's strip only draws and takes clicks,
/// with ids of its own so a document shown twice has two sets of tabs.
pub(crate) fn show(
    ui: &mut Ui,
    workspace: &Workspace,
    group: GroupId,
    strip: &mut Strip,
) -> Option<Action> {
    let focused = workspace.active_group() == group;
    let salt = (!focused).then_some(group);
    let rect = ui.max_rect();
    // Tabs scrolled out of the strip neither draw nor take clicks.
    ui.shrink_clip_rect(rect);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, STRIP);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(focus::hairline(ui.pixels_per_point()), BORDER),
    );
    let tabs = measure(ui, workspace, group);
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
        if let Some(clicked) = show_tab(ui, tab, place, salt) {
            action = Some(clicked);
        }
    }
    if focused && ui.memory(|memory| memory.has_focus(tab_id_of(workspace.active_document()))) {
        action = keyboard(ui, &tabs).or(action);
    }
    action
}

/// The active tab's keys: the tab they activate, keeping the focus here.
fn keyboard(ui: &Ui, tabs: &[Tab]) -> Option<Action> {
    let active = tabs.iter().position(|tab| tab.active)?;
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            tab_id(tabs[active].id),
            EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..EventFilter::default()
            },
        );
    });
    let target = ui.input(|input| {
        let mut target = active;
        for event in &input.events {
            let eframe::egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            if modifiers.any() {
                continue;
            }
            target = match key {
                Key::ArrowLeft | Key::ArrowUp => target.saturating_sub(1),
                Key::ArrowRight | Key::ArrowDown => (target + 1).min(tabs.len() - 1),
                Key::Home => 0,
                Key::End => tabs.len() - 1,
                _ => target,
            };
        }
        target
    });
    (target != active).then(|| Action::Select(tabs[target].id))
}

fn font() -> FontId {
    FontId::proportional(LABEL_SIZE)
}

fn measure(ui: &Ui, workspace: &Workspace, group: GroupId) -> Vec<Tab> {
    let shown = workspace.group_by_id(group);
    let active = shown.and_then(EditorGroup::active);
    shown
        .map_or(&[][..], EditorGroup::tabs)
        .iter()
        .filter_map(|&id| workspace.document(id).map(|doc| (id, doc)))
        .map(|(id, doc)| {
            let name = doc
                .path()
                .file_name()
                .map_or_else(Default::default, |name| name.to_string_lossy().into_owned());
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
                unfocused: workspace.active_group() != group,
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

/// The widget id of `id`'s tab.
pub(crate) fn tab_id(id: DocumentId) -> Id {
    Id::new(("rustode-tab", id))
}

/// The widget id of the active tab, if any.
fn tab_id_of(id: Option<DocumentId>) -> Id {
    id.map_or(Id::NULL, tab_id)
}

/// Draws one tab and its close button, exposes them, and returns a click.
/// Only the active tab and its close button take focus. `salt` is the
/// group of a strip that is not the active one.
fn show_tab(ui: &mut Ui, tab: &Tab, place: Rect, salt: Option<GroupId>) -> Option<Action> {
    let id = salt.map_or_else(|| tab_id(tab.id), |g| Id::new(("rustode-tab", g, tab.id)));
    let sense = if tab.active && salt.is_none() {
        Sense::click()
    } else {
        Sense::CLICK
    };
    let response = ui.interact(place, id, sense);
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
    let close_id = salt.map_or_else(
        || Id::new(("rustode-tab-close", tab.id)),
        |g| Id::new(("rustode-tab-close", g, tab.id)),
    );
    let close = ui
        .scope_builder(
            UiBuilder::new()
                .max_rect(slot)
                .id_salt(close_id)
                .accessibility_parent(id),
            |ui| {
                let close = ui.interact(slot, close_id, sense);
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
    let painter = ui.painter_at(ui.clip_rect().intersect(place));
    if response.has_focus() {
        focus::outline(&painter, place);
    } else if close.has_focus() {
        focus::outline(&painter, slot.shrink(4.0));
    }
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
    let (background, foreground) = if tab.active && tab.unfocused {
        (ACTIVE_BACKGROUND, UNFOCUSED_FOREGROUND)
    } else if tab.active {
        (ACTIVE_BACKGROUND, ACTIVE_FOREGROUND)
    } else {
        (STRIP, INACTIVE_FOREGROUND)
    };
    painter.rect_filled(place, 0.0, background);
    let hairline = focus::hairline(ui.pixels_per_point());
    if tab.active {
        painter.hline(
            place.x_range(),
            place.top() + 0.5,
            Stroke::new(
                hairline,
                if tab.unfocused {
                    UNFOCUSED_BORDER_TOP
                } else {
                    ACTIVE_BORDER_TOP
                },
            ),
        );
    } else {
        painter.hline(
            place.x_range(),
            place.bottom() - 0.5,
            Stroke::new(hairline, BORDER),
        );
    }
    painter.vline(
        place.right() - 0.5,
        place.y_range(),
        Stroke::new(hairline, BORDER),
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
