//! Keyboard focus across the window's parts, as in VS Code.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//!
//! - **F6 and Shift+F6** move to the next and the previous part, in VS
//!   Code's cycle editor → panel → secondary sidebar → status bar →
//!   activity bar → sidebar, skipping the parts not shown; the editor is
//!   always a target, and with nothing focused F6 goes to it
//!   (`src/vs/workbench/browser/actions/navigationActions.ts:276-345`,
//!   `:347-381`). rustode has the editor, the problems panel, the status
//!   bar and the sidebar (the explorer, the Extensions view or the Search
//!   view), so F6 goes editor → panel → status bar → sidebar, and Shift+F6
//!   the other way. The tab strip belongs to the editor part.
//! - **Tab and Shift+Tab** move through what can take focus in the order it
//!   is drawn, which is VS Code's document order: the sidebar, the active
//!   tab and its close button (only the active tab is in the Tab order,
//!   `src/vs/workbench/browser/parts/editor/multiEditorTabsControl.ts:1909-1910`),
//!   the editor, the problems panel, the status bar. The editor keeps Tab for indenting unless
//!   Ctrl+M has turned on "Tab Moves Focus"
//!   (`src/vs/editor/contrib/toggleTabFocusMode/browser/toggleTabFocusMode.ts:15-43`,
//!   with its status bar item, `src/vs/workbench/browser/parts/editor/editorStatus.ts:442-458`).
//! - **The empty editor group** takes focus when no tab is open, named
//!   "Editor Group 1 (empty)"
//!   (`src/vs/workbench/browser/parts/editor/editorGroupView.ts:557-558`, `:961`).
//! - **The focus outline** is 1 point of `focusBorder`, `#0078D4` in Dark
//!   Modern, drawn inside the focused element
//!   (`src/vs/workbench/browser/media/style.css:270-286`;
//!   `extensions/theme-defaults/themes/dark_modern.json:48`), rounded down
//!   to whole pixels as Chromium draws a 1-pixel outline.
//!
//! The AccessKit tree follows: the focused widget is the tree's focus, and
//! the status bar and the explorer point at their focused item as the
//! active descendant.

use crate::a11y;
use eframe::egui::{
    Color32, Context, Id, Painter, Rect, Sense, Stroke, StrokeKind, Ui, accesskit,
    emath::GuiRounding as _,
};

/// Dark Modern's `focusBorder`.
pub(crate) const FOCUS_BORDER: Color32 = Color32::from_rgb(0x00, 0x78, 0xd4);

/// The parts F6 moves between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Sidebar,
    /// The editor group: its tab strip and the editor.
    Editor,
    /// The problems panel below the editor.
    Panel,
    StatusBar,
}

/// The parts in the order F6 visits them: VS Code's cycle starts at the
/// editor and goes on to the panel, the status bar and the sidebar.
const CYCLE: [Part; 4] = [Part::Editor, Part::Panel, Part::StatusBar, Part::Sidebar];

/// The part F6 (`next`) or Shift+F6 goes to from `part`, skipping the
/// parts `available` refuses. The editor is always available.
pub(crate) fn neighbour(part: Part, next: bool, available: impl Fn(Part) -> bool) -> Part {
    let mut index = CYCLE.iter().position(|&p| p == part).unwrap_or(0);
    loop {
        index = if next {
            (index + 1) % CYCLE.len()
        } else {
            (index + CYCLE.len() - 1) % CYCLE.len()
        };
        let part = CYCLE[index];
        if part == Part::Editor || available(part) {
            return part;
        }
    }
}

/// The part holding the focused widget, if one has focus: the status bar
/// by its id, the sidebar and the panel by where the widget was drawn, else
/// the editor.
pub(crate) fn focused_part(
    ctx: &Context,
    sidebar: Option<Rect>,
    panel: Option<Rect>,
) -> Option<Part> {
    let id = ctx.memory(eframe::egui::Memory::focused)?;
    if id == crate::status::id() {
        return Some(Part::StatusBar);
    }
    let center = ctx.read_response(id).map(|response| response.rect.center());
    let within = |area: Option<Rect>| {
        area.zip(center)
            .is_some_and(|(area, center)| area.contains(center))
    };
    Some(if within(sidebar) {
        Part::Sidebar
    } else if within(panel) {
        Part::Panel
    } else {
        Part::Editor
    })
}

fn tab_moves_focus_id() -> Id {
    Id::new("rustode-tab-moves-focus")
}

/// Whether Tab moves focus instead of indenting in the editor (Ctrl+M).
pub(crate) fn tab_moves_focus(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp(tab_moves_focus_id()))
        .unwrap_or(false)
}

/// Ctrl+M: turns "Tab Moves Focus" on or off.
pub(crate) fn toggle_tab_moves_focus(ctx: &Context) {
    let on = !tab_moves_focus(ctx);
    ctx.data_mut(|data| data.insert_temp(tab_moves_focus_id(), on));
}

/// Whether this frame's input has Tab or Shift+Tab, which egui answers by
/// moving focus to the next or previous widget.
pub(crate) fn tab_pressed(ctx: &Context) -> bool {
    ctx.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                eframe::egui::Event::Key {
                    key: eframe::egui::Key::Tab,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.ctrl && !modifiers.command && !modifiers.alt
            )
        })
    })
}

/// The width of a 1-point line at `pixels_per_point`, on whole pixels.
pub(crate) fn hairline(pixels_per_point: f32) -> f32 {
    pixels_per_point.floor().max(1.0) / pixels_per_point
}

/// Draws the focus outline inside `rect`, on whole pixels.
pub(crate) fn outline(painter: &Painter, rect: Rect) {
    let pixels_per_point = painter.pixels_per_point();
    painter.rect_stroke(
        rect.round_to_pixels(pixels_per_point),
        0.0,
        Stroke::new(hairline(pixels_per_point), FOCUS_BORDER),
        StrokeKind::Inside,
    );
}

/// The id of the empty editor group.
pub(crate) fn empty_group_id() -> Id {
    Id::new("rustode-empty-group")
}

/// What the empty editor group says to screen readers.
pub(crate) const EMPTY_GROUP: &str = "Editor Group 1 (empty)";

/// Makes the editor area with no tab open focusable, as VS Code's empty
/// group is: a click or F6 focuses it, and it shows the focus outline.
pub(crate) fn empty_group(ui: &mut Ui, rect: Rect) {
    let response = ui.interact(rect, empty_group_id(), Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    ui.ctx().accesskit_node_builder(empty_group_id(), |node| {
        node.set_role(accesskit::Role::Group);
        node.set_label(EMPTY_GROUP);
        node.set_bounds(a11y::bounds(rect));
    });
    if response.has_focus() {
        outline(ui.painter(), rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f6_cycles_like_vs_code_and_skips_hidden_parts() {
        let all = |_: Part| true;
        assert_eq!(neighbour(Part::Editor, true, all), Part::Panel);
        assert_eq!(neighbour(Part::Panel, true, all), Part::StatusBar);
        assert_eq!(neighbour(Part::StatusBar, true, all), Part::Sidebar);
        assert_eq!(neighbour(Part::Sidebar, true, all), Part::Editor);
        assert_eq!(neighbour(Part::Editor, false, all), Part::Sidebar);
        assert_eq!(neighbour(Part::Sidebar, false, all), Part::StatusBar);
        assert_eq!(neighbour(Part::StatusBar, false, all), Part::Panel);
        assert_eq!(neighbour(Part::Panel, false, all), Part::Editor);
        let no_sidebar = |part: Part| part != Part::Sidebar;
        assert_eq!(neighbour(Part::StatusBar, true, no_sidebar), Part::Editor);
        assert_eq!(neighbour(Part::Editor, false, no_sidebar), Part::StatusBar);
        let no_panel = |part: Part| part != Part::Panel;
        assert_eq!(neighbour(Part::Editor, true, no_panel), Part::StatusBar);
        let none = |_: Part| false;
        assert_eq!(neighbour(Part::Editor, true, none), Part::Editor);
    }

    #[test]
    fn hairlines_are_whole_pixels() {
        assert!((hairline(1.0) - 1.0).abs() < f32::EPSILON);
        assert!((hairline(1.25) * 1.25 - 1.0).abs() < 1e-6);
        assert!((hairline(1.5) * 1.5 - 1.0).abs() < 1e-6);
        assert!((hairline(2.0) * 2.0 - 2.0).abs() < 1e-6);
    }
}
