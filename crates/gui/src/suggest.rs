//! The hover and suggest overlays of the editor: the keys that open and
//! drive them, and how they are drawn, anchored to the text. What they hold,
//! how the list is filtered and ordered and what accepting does are the
//! workspace's (`Workspace::completion_list`, `hover`, ...); this module maps
//! input and draws.
//!
//! Studied in VS Code, see `ATTRIBUTION.md`:
//! - keys (`src/vs/editor/contrib/suggest/browser/suggestController.ts:797-970`):
//!   Ctrl+Space opens the list, Enter or Tab accepts, Escape closes, Up and
//!   Down move (wrapping), Page Up and Page Down move by a page;
//! - Ctrl+K Ctrl+I shows the hover at the cursor
//!   (`src/vs/editor/contrib/hover/browser/hoverActions.ts:28-70`), and the
//!   pointer asks for one after resting on a char for `editor.hover.delay`,
//!   300 ms (`src/vs/editor/common/config/editorOptions.ts:2405`);
//! - colors: the widget background is Dark Modern's `editorWidget.background`
//!   and its border `widget.border`
//!   (`extensions/theme-defaults/themes/dark_modern.json:46`, `:133`), the
//!   selected row `list.activeSelectionBackground`
//!   (`src/vs/platform/theme/common/colors/listColors.ts:33-34`).
//!
//! The list goes under the word being completed, or above it when there is no
//! room; the hover above the char, or under it. A click on a row accepts it.
//!
//! AccessKit: the list is a list box named "Suggestions" whose options are
//! the visible rows, the selected one its active descendant; the hover is a
//! tooltip labelled with its text.

use crate::a11y;
use eframe::egui::{
    self, Area, Color32, FontId, Id, Key, Modifiers, Order, Pos2, Rect, Sense, Stroke, StrokeKind,
    Ui, UiBuilder, Vec2, accesskit,
    text::{LayoutJob, TextFormat},
};
use rustode_workspace::{CompletionList, CompletionMove, CompletionRow, DocumentId, Workspace};

/// Rows the list shows at most.
pub(crate) const LIST_ROWS: usize = 10;
/// Seconds the pointer rests on a char before the hover is asked for.
pub(crate) const HOVER_DELAY: f64 = 0.3;
const ROW_HEIGHT: f32 = 22.0;
const FONT_SIZE: f32 = 13.0;
const PADDING: f32 = 6.0;
const MAX_LIST_WIDTH: f32 = 520.0;
const MAX_HOVER_WIDTH: f32 = 480.0;
/// Lines of hover text shown.
const HOVER_LINES: usize = 12;

const BACKGROUND: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
const BORDER: Color32 = Color32::from_rgb(0x31, 0x31, 0x31);
const SELECTED: Color32 = Color32::from_rgb(0x04, 0x39, 0x5e);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const DIM: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);
const MATCH: Color32 = Color32::from_rgb(0x2a, 0xaa, 0xff);

/// What the overlays remember between frames.
#[derive(Debug, Default)]
pub(crate) struct State {
    /// Ctrl+K was pressed: the next key completes the chord.
    chord: bool,
    /// The char the pointer rests on, and since when.
    rest: Option<Rest>,
}

#[derive(Debug, Clone, Copy)]
struct Rest {
    offset: usize,
    since: f64,
    asked: bool,
}

fn list_id() -> Id {
    Id::new("rustode-suggest-list")
}

fn row_id(index: usize) -> Id {
    Id::new(("rustode-suggest-row", index))
}

/// Whether the pointer is over the list: a click on a row takes the focus
/// from the editor, and that is not the editor losing it to somewhere else.
pub(crate) fn pointer_on_list(ui: &Ui) -> bool {
    let ctx = ui.ctx();
    ctx.pointer_latest_pos()
        .and_then(|pos| ctx.layer_id_at(pos))
        .is_some_and(|layer| layer == egui::LayerId::new(Order::Foreground, list_area()))
}

fn list_area() -> Id {
    Id::new("rustode-suggest")
}

fn hover_id() -> Id {
    Id::new("rustode-hover")
}

fn consume(ui: &Ui, modifiers: Modifiers, key: Key) -> bool {
    ui.input_mut(|input| input.consume_key(modifiers, key))
}

/// What the overlays' keys did.
#[derive(Debug, Default)]
pub(crate) struct Keys {
    /// Whether a key was taken.
    pub acted: bool,
    /// Why an accept failed, for the status bar.
    pub error: Option<String>,
}

/// Takes the keys that belong to the overlays out of this frame's input, and
/// acts on them.
pub(crate) fn keys(ui: &Ui, workspace: &mut Workspace, state: &mut State) -> Keys {
    let mut keys = Keys::default();
    if workspace.completion_visible() {
        for (key, movement) in [
            (Key::ArrowDown, CompletionMove::Next),
            (Key::ArrowUp, CompletionMove::Previous),
            (Key::PageDown, CompletionMove::PageDown(LIST_ROWS)),
            (Key::PageUp, CompletionMove::PageUp(LIST_ROWS)),
        ] {
            if consume(ui, Modifiers::NONE, key) {
                workspace.completion_move(movement);
                keys.acted = true;
            }
        }
        if consume(ui, Modifiers::NONE, Key::Enter) || consume(ui, Modifiers::NONE, Key::Tab) {
            keys.error = workspace.completion_accept().err().map(|e| e.to_string());
            keys.acted = true;
        }
        if consume(ui, Modifiers::NONE, Key::Escape) {
            workspace.dismiss_completion();
            keys.acted = true;
        }
    }
    if consume(ui, Modifiers::COMMAND, Key::Space) {
        drop(workspace.execute("editor.action.triggerSuggest"));
        keys.acted = true;
    }
    let mut began = false;
    if state.chord && consume(ui, Modifiers::COMMAND, Key::I) {
        state.chord = false;
        drop(workspace.execute("editor.action.showHover"));
        keys.acted = true;
    } else if consume(ui, Modifiers::COMMAND, Key::K) {
        workspace.hide_hover();
        state.chord = true;
        began = true;
        keys.acted = true;
    }
    let typed = ui.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key { pressed: true, .. } | egui::Event::Text(_)
            )
        })
    });
    if typed {
        // Any other key ends the chord.
        state.chord &= began;
        workspace.hide_hover();
    }
    keys
}

/// What the pointer wants done to the hover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pointer {
    /// Nothing.
    Idle,
    /// Hide the hover: the pointer left the char it showed.
    Hide,
    /// Ask for the hover of this char.
    Ask(usize),
}

/// Follows the pointer: after it rests on a char for [`HOVER_DELAY`], the
/// hover of that char is asked for; moving to another char hides it.
/// `target` is the char under the pointer, `None` when it is not over text.
pub(crate) fn pointer(
    ui: &Ui,
    workspace: &Workspace,
    state: &mut State,
    target: Option<usize>,
) -> Pointer {
    let now = ui.input(|input| input.time);
    let Some(offset) = target else {
        return if state.rest.take().is_some() {
            Pointer::Hide
        } else {
            Pointer::Idle
        };
    };
    let mut action = Pointer::Idle;
    let rest = match state.rest {
        Some(rest) if rest.offset == offset => rest,
        previous => {
            if previous.is_some() {
                action = Pointer::Hide;
            }
            Rest {
                offset,
                since: now,
                asked: false,
            }
        }
    };
    let waited = now - rest.since;
    let ask = !rest.asked && waited >= HOVER_DELAY && !workspace.completion_visible();
    if ask {
        action = Pointer::Ask(offset);
    } else if !rest.asked {
        let left = (HOVER_DELAY - waited).max(0.0);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(left));
    }
    state.rest = Some(Rest {
        asked: rest.asked || ask,
        ..rest
    });
    action
}

/// Where the text is: the area the overlays stay inside, and the top left of
/// a char, which is on screen or not.
pub(crate) struct Anchors<'a> {
    pub clip: Rect,
    pub at: &'a dyn Fn(usize) -> Option<Pos2>,
    pub line_height: f32,
}

/// Draws the list, or else the hover. Returns the row of the list that was
/// clicked, as an index in the whole list.
pub(crate) fn show(
    ui: &Ui,
    workspace: &Workspace,
    document: DocumentId,
    anchors: &Anchors<'_>,
) -> Option<usize> {
    if let Some(list) = workspace.completion_list(LIST_ROWS) {
        return if list.document == document {
            show_list(ui, &list, anchors)
        } else {
            None
        };
    }
    let hover = workspace.hover().filter(|h| h.document == document)?;
    show_hover(ui, hover.anchor(), &hover.text, anchors);
    None
}

/// Fits a popup of `size` whose anchor char is at `at`: left aligned with
/// it, kept inside `clip`; below the line, or above it when there is no
/// room (`above_first` tries above first).
fn place(at: Pos2, size: Vec2, anchors: &Anchors<'_>, above_first: bool) -> Pos2 {
    let clip = anchors.clip;
    let x = at.x.min(clip.right() - size.x).max(clip.left());
    let below = at.y + anchors.line_height + 2.0;
    let above = at.y - size.y - 2.0;
    let fits_below = below + size.y <= clip.bottom();
    let fits_above = above >= clip.top();
    let y = if (above_first && fits_above) || !fits_below {
        above.max(clip.top())
    } else {
        below
    };
    Pos2::new(x, y)
}

fn show_list(ui: &Ui, list: &CompletionList, anchors: &Anchors<'_>) -> Option<usize> {
    let at = (anchors.at)(list.start)?;
    let ctx = ui.ctx().clone();
    let galleys: Vec<_> = list
        .rows
        .iter()
        .map(|row| ctx.fonts_mut(|fonts| fonts.layout_job(row_job(row))))
        .collect();
    let width = galleys
        .iter()
        .map(|g| g.size().x)
        .fold(0.0, f32::max)
        .mul_add(1.0, 2.0 * PADDING)
        .clamp(120.0, MAX_LIST_WIDTH.min(anchors.clip.width()));
    let height = ROW_HEIGHT * crate::editor::float(list.rows.len());
    let size = Vec2::new(width, height);
    let pos = place(at, size, anchors, false);
    let mut clicked = None;
    Area::new(list_area())
        .order(Order::Foreground)
        .fixed_pos(pos)
        .interactable(true)
        .show(&ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 2.0, BACKGROUND);
            ui.interact(rect, list_id(), Sense::hover());
            ctx.accesskit_node_builder(list_id(), |node| {
                node.set_role(accesskit::Role::ListBox);
                node.set_label("Suggestions");
                node.set_bounds(a11y::bounds(rect));
            });
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(rect)
                    .id_salt("rustode-suggest-rows")
                    .accessibility_parent(list_id()),
                |ui| {
                    for (index, (row, galley)) in list.rows.iter().zip(&galleys).enumerate() {
                        let place = Rect::from_min_size(
                            rect.min + Vec2::new(0.0, ROW_HEIGHT * crate::editor::float(index)),
                            Vec2::new(width, ROW_HEIGHT),
                        );
                        let absolute = list.first + index;
                        let selected = absolute == list.selected;
                        let response = ui.interact(place, row_id(index), Sense::CLICK);
                        if response.clicked() {
                            clicked = Some(absolute);
                        }
                        if selected {
                            ui.painter().rect_filled(place, 0.0, SELECTED);
                        }
                        let top = place.top() + (ROW_HEIGHT - galley.size().y) / 2.0;
                        ui.painter().galley(
                            Pos2::new(place.left() + PADDING, top),
                            galley.clone(),
                            FOREGROUND,
                        );
                        ctx.accesskit_node_builder(row_id(index), |node| {
                            node.set_role(accesskit::Role::ListBoxOption);
                            node.set_label(option_label(row));
                            node.set_selected(selected);
                            node.set_bounds(a11y::bounds(place));
                        });
                        if selected {
                            ctx.accesskit_node_builder(list_id(), |node| {
                                node.set_active_descendant(row_id(index).accesskit_id());
                            });
                        }
                    }
                },
            );
            ui.painter()
                .rect_stroke(rect, 2.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
        });
    clicked
}

/// What a screen reader says for a row: the label, then its kind and detail.
fn option_label(row: &CompletionRow) -> String {
    match &row.detail {
        Some(detail) => format!("{}, {}, {detail}", row.label, row.kind.label()),
        None => format!("{}, {}", row.label, row.kind.label()),
    }
}

/// A row as laid out: the kind's letter, the label with its matched chars
/// colored, the detail dim.
fn row_job(row: &CompletionRow) -> LayoutJob {
    let font = FontId::monospace(FONT_SIZE);
    let format = |color| TextFormat::simple(font.clone(), color);
    let mut job = LayoutJob {
        break_on_newline: false,
        ..LayoutJob::default()
    };
    job.append(&format!("{}  ", row.kind.letter()), 0.0, format(DIM));
    for (index, c) in row.label.chars().enumerate() {
        let color = if row.matched.contains(&index) {
            MATCH
        } else {
            FOREGROUND
        };
        job.append(&c.to_string(), 0.0, format(color));
    }
    if let Some(detail) = &row.detail {
        job.append(&format!("   {detail}"), 0.0, format(DIM));
    }
    job
}

fn show_hover(ui: &Ui, anchor: usize, text: &str, anchors: &Anchors<'_>) {
    let Some(at) = (anchors.at)(anchor) else {
        return;
    };
    let ctx = ui.ctx().clone();
    let shown: Vec<&str> = text.lines().take(HOVER_LINES).collect();
    let wrap = MAX_HOVER_WIDTH.min(anchors.clip.width()) - 2.0 * PADDING;
    let galley = ctx.fonts_mut(|fonts| {
        fonts.layout(
            shown.join("\n"),
            FontId::proportional(FONT_SIZE),
            FOREGROUND,
            wrap,
        )
    });
    let size = galley.size() + Vec2::splat(2.0 * PADDING);
    let pos = place(at, size, anchors, true);
    Area::new(hover_id())
        .order(Order::Tooltip)
        .fixed_pos(pos)
        .interactable(false)
        .show(&ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(rect, 2.0, BACKGROUND);
            ui.painter()
                .galley(rect.min + Vec2::splat(PADDING), galley, FOREGROUND);
            ui.painter()
                .rect_stroke(rect, 2.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
            ui.interact(rect, hover_id().with("node"), Sense::hover());
            ctx.accesskit_node_builder(hover_id().with("node"), |node| {
                node.set_role(accesskit::Role::Tooltip);
                node.set_label(text.to_owned());
                node.set_bounds(a11y::bounds(rect));
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchors(at: &dyn Fn(usize) -> Option<Pos2>) -> Anchors<'_> {
        Anchors {
            clip: Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(400.0, 200.0)),
            at,
            line_height: 19.0,
        }
    }

    #[test]
    fn a_popup_goes_under_the_line_or_above_it_and_stays_inside() {
        let at = |_| None;
        let anchors = anchors(&at);
        let size = Vec2::new(100.0, 50.0);
        let below = place(Pos2::new(380.0, 20.0), size, &anchors, false);
        assert_eq!((below.x, below.y), (300.0, 41.0), "pulled in from the edge");
        let above = place(Pos2::new(10.0, 180.0), size, &anchors, false);
        assert_eq!(above.y, 128.0, "no room below");
        let hover = place(Pos2::new(10.0, 100.0), size, &anchors, true);
        assert_eq!(hover.y, 48.0, "a hover prefers above");
        let low = place(Pos2::new(10.0, 10.0), size, &anchors, true);
        assert_eq!(low.y, 31.0, "and goes below when there is no room above");
    }

    #[test]
    fn rows_read_as_label_kind_and_detail() {
        let row = CompletionRow {
            label: "print".into(),
            kind: rustode_workspace::CompletionKind::Function,
            detail: Some("fn print()".into()),
            matched: vec![0],
        };
        assert_eq!(option_label(&row), "print, function, fn print()");
    }
}
