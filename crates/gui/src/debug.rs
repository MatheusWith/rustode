//! The debug panel below the editor while a session runs: CALL STACK,
//! VARIABLES, WATCH and DEBUG CONSOLE side by side, as VS Code's debug views
//! and Debug Console list them
//! (`src/vs/workbench/contrib/debug/browser/callStackView.ts`,
//! `variablesView.ts`, `watchExpressionsView.ts`, `repl.ts`; studied and
//! reimplemented, see `ATTRIBUTION.md`). All state is the workspace's; the
//! panel keeps only which column to focus and the text being typed.
//!
//! A row is a button: a click, or Enter once focused, picks the frame or
//! opens and closes the variable; Up and Down move between the rows of a
//! column, Right and Left open and close a variable, Delete removes the
//! watch expression. The Watch column ends in a field that adds an
//! expression on Enter, the console in one that evaluates it.

use eframe::egui::{
    self, Color32, FontId, Id, Key, Pos2, Rect, RichText, Sense, Stroke, TextEdit, Ui, UiBuilder,
    Vec2, WidgetInfo, WidgetType,
};
use rustode_workspace::{ConsoleKind, DebugState, DebugTree, VariableRow, Workspace};

/// Height of the panel, in points.
pub const PANEL_HEIGHT: f32 = 200.0;

const BACKGROUND: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const DESCRIPTION: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);
/// `debugTokenExpression.name`, `.value` and `.error` of Dark Modern
/// (`src/vs/workbench/contrib/debug/browser/debugColors.ts`).
const NAME: Color32 = Color32::from_rgb(0xc5, 0x86, 0xc0);
const VALUE: Color32 = Color32::from_rgb(0xb5, 0xce, 0xa8);
const ERROR: Color32 = Color32::from_rgb(0xf1, 0x4c, 0x4c);
const SELECTED: Color32 = Color32::from_rgb(0x04, 0x39, 0x5e);
const HOVERED: Color32 = Color32::from_rgb(0x2a, 0x2d, 0x2e);
const FONT_SIZE: f32 = 13.0;
const ROW_HEIGHT: f32 = 20.0;
const INDENT: f32 = 14.0;

/// A column of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Column {
    CallStack,
    Variables,
    Watch,
    Console,
}

/// The panel's state, kept while it is hidden.
#[derive(Debug, Default)]
pub(crate) struct Panel {
    /// Shown even with no session, after a focus command.
    pub pinned: bool,
    /// The column to focus on the next frame.
    pub focus: Option<Column>,
    watch_input: String,
    console_input: String,
    /// The watch expression whose row had focus on the last frame.
    focused_watch: Option<usize>,
}

/// What the panel did this frame.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// Why a command was refused.
    pub error: Option<String>,
}

impl Panel {
    /// Shows the panel and focuses `column` on the next frame.
    pub(crate) fn focus(&mut self, column: Column) {
        self.pinned = true;
        self.focus = Some(column);
    }

    /// Removes the watch expression whose row had focus; returns whether
    /// there was one.
    pub(crate) fn remove_focused_watch(&mut self, workspace: &mut Workspace) -> bool {
        self.focused_watch
            .take()
            .is_some_and(|root| workspace.remove_watch(root))
    }
}

/// Whether the panel is shown: while there is a session, or when pinned.
pub(crate) fn visible(panel: &Panel, workspace: &Workspace) -> bool {
    panel.pinned || workspace.debug_status().state != DebugState::Inactive
}

/// The id of the console's field, the panel's first stop for F6.
pub(crate) fn console_id() -> Id {
    Id::new("rustode-debug-console")
}

fn watch_id() -> Id {
    Id::new("rustode-debug-watch-input")
}

fn row_id(column: Column, index: usize) -> Id {
    Id::new(("rustode-debug-row", column as u8, index))
}

/// Draws the panel over the whole of `ui`.
pub(crate) fn show(ui: &mut Ui, workspace: &mut Workspace, panel: &mut Panel) -> Outcome {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, BACKGROUND);
    ui.painter()
        .hline(rect.x_range(), rect.top(), Stroke::new(1.0, BORDER));
    let inner = rect.shrink2(Vec2::new(8.0, 4.0));
    let widths = [0.28, 0.26, 0.20, 0.26];
    let mut x = inner.left();
    let mut outcome = Outcome::default();
    let focus = panel.focus.take();
    panel.focused_watch = None;
    for (index, (column, title)) in [
        (Column::CallStack, "CALL STACK"),
        (Column::Variables, "VARIABLES"),
        (Column::Watch, "WATCH"),
        (Column::Console, "DEBUG CONSOLE"),
    ]
    .into_iter()
    .enumerate()
    {
        let width = inner.width() * widths[index];
        let area = Rect::from_min_size(
            Pos2::new(x, inner.top()),
            Vec2::new(width - 8.0, inner.height()),
        );
        x += width;
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(area)
                .id_salt(("rustode-debug-column", index)),
        );
        ui.set_clip_rect(area.intersect(ui.clip_rect()));
        ui.label(
            RichText::new(title)
                .font(FontId::proportional(11.0))
                .color(FOREGROUND),
        );
        let error = match column {
            Column::CallStack => call_stack(&mut ui, workspace, focus == Some(column)),
            Column::Variables => tree(
                &mut ui,
                workspace,
                DebugTree::Variables,
                focus == Some(column),
            ),
            Column::Watch => watch(&mut ui, workspace, panel, focus == Some(column)),
            Column::Console => console(&mut ui, workspace, panel, focus == Some(column)),
        };
        outcome.error = outcome.error.take().or(error);
    }
    outcome
}

/// One selectable row; returns its response. `selected` paints the
/// frame in use.
fn row(
    ui: &mut Ui,
    id: Id,
    selected: bool,
    paint: impl FnOnce(&mut Ui, Rect),
    spoken: &str,
) -> egui::Response {
    let (_, rect) = ui.allocate_space(Vec2::new(ui.available_width(), ROW_HEIGHT));
    let response = ui.interact(rect, id, Sense::click());
    if selected || response.has_focus() {
        ui.painter().rect_filled(rect, 0.0, SELECTED);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, HOVERED);
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    paint(&mut child, rect);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, spoken));
    response
}

/// Paints `text` at `at` (left, vertically centered); returns its width.
fn text_at(ui: &Ui, at: Pos2, text: &str, color: Color32, bold: bool) -> f32 {
    let mut font = FontId::proportional(FONT_SIZE);
    if bold {
        font = FontId::proportional(FONT_SIZE + 0.5);
    }
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let size = galley.size();
    ui.painter()
        .galley(Pos2::new(at.x, at.y - size.y / 2.0), galley, color);
    size.x
}

/// Moves focus between the rows of a column on Up and Down.
fn arrows(ui: &Ui, response: &egui::Response, column: Column, index: usize, count: usize) {
    if !response.has_focus() {
        return;
    }
    let (up, down) = ui.input_mut(|input| {
        (
            input.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            input.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
        )
    });
    if up && index > 0 {
        ui.memory_mut(|m| m.request_focus(row_id(column, index - 1)));
    }
    if down && index + 1 < count {
        ui.memory_mut(|m| m.request_focus(row_id(column, index + 1)));
    }
}

fn call_stack(ui: &mut Ui, workspace: &mut Workspace, focus: bool) -> Option<String> {
    let status = workspace.debug_status();
    let frames = workspace.debug_frames();
    let current = workspace.debug_selected_frame();
    let mut picked = None;
    egui::ScrollArea::vertical()
        .id_salt("rustode-debug-stack")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for thread in workspace.debug_threads() {
                let stopped =
                    status.state == DebugState::Stopped && status.thread == Some(thread.id);
                let state = if stopped {
                    format!(
                        "PAUSED ON {}",
                        status.reason.as_deref().unwrap_or("stop").to_uppercase()
                    )
                } else if status.state == DebugState::Stopped {
                    String::new()
                } else {
                    "RUNNING".to_owned()
                };
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&thread.name)
                            .font(FontId::proportional(FONT_SIZE))
                            .color(FOREGROUND)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(state)
                            .font(FontId::proportional(11.0))
                            .color(DESCRIPTION),
                    );
                });
                if !stopped {
                    continue;
                }
                for (index, frame) in frames.iter().enumerate() {
                    let place = frame.path.as_ref().map_or_else(String::new, |path| {
                        let name = path
                            .file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                        format!("{name}:{}", frame.line + 1)
                    });
                    let spoken = format!("{} {place}", frame.name);
                    let response = row(
                        ui,
                        row_id(Column::CallStack, index),
                        index == current,
                        |ui, rect| {
                            let at = Pos2::new(rect.left() + INDENT, rect.center().y);
                            let used = text_at(ui, at, &frame.name, FOREGROUND, false);
                            text_at(
                                ui,
                                Pos2::new(at.x + used + 8.0, at.y),
                                &place,
                                DESCRIPTION,
                                false,
                            );
                        },
                        &spoken,
                    );
                    if focus && index == 0 {
                        response.request_focus();
                    }
                    arrows(ui, &response, Column::CallStack, index, frames.len());
                    if response.clicked() {
                        picked = Some(index);
                    }
                }
            }
        });
    picked
        .and_then(|index| workspace.select_debug_frame(index).err())
        .map(|err| err.to_string())
}

/// The text of a variable row for screen readers.
fn spoken_variable(row: &VariableRow) -> String {
    if row.value.is_empty() {
        row.name.clone()
    } else {
        format!("{}, value {}", row.name, row.value)
    }
}

/// One row of a variables or watch tree: an arrow, the name and the value.
fn variable_row(
    ui: &mut Ui,
    tree: DebugTree,
    index: usize,
    count: usize,
    v: &VariableRow,
) -> (egui::Response, bool) {
    let column = if tree == DebugTree::Variables {
        Column::Variables
    } else {
        Column::Watch
    };
    let mut removed = false;
    let response = row(
        ui,
        row_id(column, index),
        false,
        |ui, rect| {
            let x = rect.left() + INDENT * to_f32(v.depth);
            let y = rect.center().y;
            if v.expandable {
                let arrow = if v.expanded { "v" } else { ">" };
                text_at(ui, Pos2::new(x, y), arrow, DESCRIPTION, false);
            }
            let mut at = Pos2::new(x + INDENT, y);
            let used = text_at(ui, at, &v.name, NAME, false);
            at.x += used;
            if !v.value.is_empty() {
                let colon = text_at(ui, at, ": ", FOREGROUND, false);
                at.x += colon;
                let shown = if v.loading {
                    format!("{} ...", v.value)
                } else {
                    v.value.clone()
                };
                text_at(ui, at, &shown, VALUE, false);
            }
            if tree == DebugTree::Watch && v.depth == 0 {
                let center = Pos2::new(rect.right() - 8.0, y);
                let d = 3.0;
                let stroke = Stroke::new(1.0, DESCRIPTION);
                ui.painter().line_segment(
                    [center + Vec2::new(-d, -d), center + Vec2::new(d, d)],
                    stroke,
                );
                ui.painter().line_segment(
                    [center + Vec2::new(-d, d), center + Vec2::new(d, -d)],
                    stroke,
                );
            }
        },
        &spoken_variable(v),
    );
    arrows(ui, &response, column, index, count);
    if tree == DebugTree::Watch && v.depth == 0 && response.has_focus() {
        removed = ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Delete));
    }
    (response, removed)
}

/// A float for a small count.
fn to_f32(value: usize) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

fn tree(ui: &mut Ui, workspace: &mut Workspace, tree: DebugTree, focus: bool) -> Option<String> {
    let rows = workspace.debug_rows(tree);
    let mut toggled = None;
    egui::ScrollArea::vertical()
        .id_salt(("rustode-debug-tree", tree as u8))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            toggle_rows(ui, tree, &rows, focus, &mut toggled);
        });
    toggled
        .and_then(|index| workspace.toggle_debug_row(tree, index).err())
        .map(|err| err.to_string())
}

/// Draws `rows`; the row that was activated, or opened or closed with the
/// arrow keys, goes to `toggled`.
fn toggle_rows(
    ui: &mut Ui,
    tree: DebugTree,
    rows: &[VariableRow],
    focus: bool,
    toggled: &mut Option<usize>,
) {
    for (index, v) in rows.iter().enumerate() {
        let (response, _) = variable_row(ui, tree, index, rows.len(), v);
        if focus && index == 0 {
            response.request_focus();
        }
        let (right, left) = if response.has_focus() {
            ui.input_mut(|input| {
                (
                    input.consume_key(egui::Modifiers::NONE, Key::ArrowRight),
                    input.consume_key(egui::Modifiers::NONE, Key::ArrowLeft),
                )
            })
        } else {
            (false, false)
        };
        let by_key = (right && !v.expanded && v.expandable) || (left && v.expanded);
        if response.clicked() || by_key {
            *toggled = Some(index);
        }
    }
}

fn watch(ui: &mut Ui, workspace: &mut Workspace, panel: &mut Panel, focus: bool) -> Option<String> {
    let rows = workspace.debug_rows(DebugTree::Watch);
    let mut toggled = None;
    let mut removed = None;
    let height = ui.available_height() - ROW_HEIGHT - 4.0;
    egui::ScrollArea::vertical()
        .id_salt("rustode-debug-watch")
        .max_height(height.max(ROW_HEIGHT))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, v) in rows.iter().enumerate() {
                let (response, delete) = variable_row(ui, DebugTree::Watch, index, rows.len(), v);
                if focus && index == 0 {
                    response.request_focus();
                }
                if response.has_focus() {
                    panel.focused_watch = Some(v.root);
                }
                if delete || (v.depth == 0 && cross_clicked(ui, &response)) {
                    removed = Some(v.root);
                } else if response.clicked() {
                    toggled = Some(index);
                }
            }
        });
    if let Some(root) = removed {
        workspace.remove_watch(root);
    }
    let mut error = toggled
        .and_then(|index| workspace.toggle_debug_row(DebugTree::Watch, index).err())
        .map(|err| err.to_string());
    let field = ui.add(
        TextEdit::singleline(&mut panel.watch_input)
            .id(watch_id())
            .hint_text("Add expression")
            .desired_width(ui.available_width()),
    );
    if focus && rows.is_empty() {
        field.request_focus();
    }
    if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
        match workspace.add_watch(&panel.watch_input) {
            Ok(()) => panel.watch_input.clear(),
            Err(err) => error = Some(err.to_string()),
        }
        field.request_focus();
    }
    error
}

/// Whether the click on a watch row landed on its cross.
fn cross_clicked(ui: &Ui, response: &egui::Response) -> bool {
    response.clicked()
        && ui
            .input(|input| input.pointer.interact_pos())
            .is_some_and(|pos| pos.x > response.rect.right() - 18.0)
}

fn console(
    ui: &mut Ui,
    workspace: &mut Workspace,
    panel: &mut Panel,
    focus: bool,
) -> Option<String> {
    let height = ui.available_height() - ROW_HEIGHT - 4.0;
    egui::ScrollArea::vertical()
        .id_salt("rustode-debug-console-lines")
        .max_height(height.max(ROW_HEIGHT))
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in workspace.debug_console() {
                let (text, color) = match line.kind {
                    ConsoleKind::Input => (format!("> {}", line.text), DESCRIPTION),
                    ConsoleKind::Output | ConsoleKind::Result => (line.text.clone(), FOREGROUND),
                    ConsoleKind::Error => (line.text.clone(), ERROR),
                };
                ui.label(
                    RichText::new(text)
                        .font(FontId::monospace(12.0))
                        .color(color),
                );
            }
        });
    let field = ui.add(
        TextEdit::singleline(&mut panel.console_input)
            .id(console_id())
            .hint_text("Evaluate an expression")
            .desired_width(ui.available_width()),
    );
    if focus {
        field.request_focus();
    }
    if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
        field.request_focus();
        return match workspace.evaluate_in_console(&panel.console_input) {
            Ok(()) => {
                panel.console_input.clear();
                None
            }
            Err(err) => Some(err.to_string()),
        };
    }
    None
}
