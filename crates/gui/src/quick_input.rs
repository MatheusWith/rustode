//! The quick input box: quick open (Ctrl+P), the command palette
//! (Ctrl+Shift+P, F1) and go to line (Ctrl+G) share it, as in VS Code and
//! the TUI. Its rows come from the workspace: `parse_quick_query` reads the
//! text (`>` lists commands, `:` goes to a line), `quick_open` ranks the
//! files and `commands` lists the command registry; the window runs the
//! chosen row (`app.rs`).
//!
//! In the AccessKit tree the box is an editable combo box named after what
//! it searches, which controls a list box of options; the selected option
//! is selected and is the combo box's active descendant, as VS Code's input
//! points at its list (`src/vs/platform/quickinput/browser/quickInputController.ts:274-289`,
//! `quickInputBox.ts:147-162`; `quickInputList.ts:307-311`).
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//!
//! - `min(600, 0.62 × width)` points wide and its list at most 40 % of the
//!   height of the window, 6 points from the top
//!   (`src/vs/platform/quickinput/browser/quickInputController.ts:69`,
//!   `:1035-1038`, `:1094-1095`; `src/vs/workbench/browser/layout.ts:255-277`),
//!   centered over the editor area here rather than the window; a 26-point
//!   field in a header padded 6, 6, 4 and 6 points (`quickInputController.ts:1047`;
//!   `src/vs/platform/quickinput/browser/media/quickInput.css:160-167`);
//!   22-point rows, at most 20, padded 6
//!   points twice, with 3-point corners and 7 points under the list
//!   (`quickInput.css:252-288`); 12-point corners (`:12`,
//!   `src/vs/platform/theme/common/sizes/baseSizes.ts:102-108`);
//! - Up and Down wrap around the rows (`quickInputList.ts:929`,
//!   `quickInput.ts:1093`), Page Up and Page Down move by the rows in view,
//!   Enter runs the selected row, Escape closes and gives focus back, and
//!   the box closes when it loses focus;
//! - matched chars in `list.highlightForeground`, descriptions dimmed to
//!   70 % and 0.9 em, keybindings drawn as keys
//!   (`quickInput.css:347-377`; `src/vs/platform/theme/common/colors/listColors.ts:65-66`,
//!   `:81-83`; `src/vs/base/browser/ui/iconLabel/iconlabel.css:83-87`,
//!   `:136-139`; `src/vs/base/browser/ui/keybindingLabel/keybindingLabel.css:9-30`;
//!   `src/vs/platform/theme/common/colors/inputColors.ts:212-225`) on Dark
//!   Modern's quick input colors (`extensions/theme-defaults/themes/dark_modern.json:28`,
//!   `:77-78`, `:133`) and the focused row's
//!   (`src/vs/platform/theme/common/colors/quickpickColors.ts:41-55`);
//! - the texts: "Type the name of a command to run."
//!   (`src/vs/workbench/contrib/quickaccess/browser/quickAccess.contribution.ts:46`),
//!   "No matching commands" (`commandsQuickAccess.ts:86`), "No matching
//!   results" (`src/vs/workbench/contrib/search/browser/anythingQuickAccess.ts:111`),
//!   and go to line's name and prompts
//!   (`src/vs/editor/contrib/quickAccess/browser/gotoLineQuickAccess.ts:64`,
//!   `:160`, `:208`, `:227`, `:240`). Quick open's hint is VS Code's
//!   (`src/vs/workbench/contrib/search/browser/searchQuickAccess.contribution.ts:21`)
//!   without symbols and links, which rustode does not have yet.

use crate::a11y;
use crate::input::{self, Caret, FOREGROUND, Field, Panel};
use eframe::egui::{
    self, Color32, Context, CornerRadius, Event, FontId, Galley, Id, Key, Rect, Sense, Shape,
    Stroke, StrokeKind, Ui, UiBuilder, accesskit, pos2,
    text::{LayoutJob, TextFormat},
    vec2,
};
use rustode_workspace::{
    CommandMatch, Document, FileMatch, QuickQuery, Workspace, parse_quick_query,
};
use std::path::PathBuf;
use std::sync::Arc;

const MAX_WIDTH: f32 = 600.0;
/// VS Code's "golden cut" of the width.
const WIDTH_RATIO: f32 = 0.62;
const TOP: f32 = 6.0;
const PADDING: f32 = 6.0;
const INPUT_HEIGHT: f32 = 26.0;
/// The field and the padding around it.
const HEADER: f32 = PADDING + INPUT_HEIGHT + 4.0;
/// Height of one row.
const ROW_HEIGHT: f32 = 22.0;
const MAX_ROWS: usize = 20;
const HEIGHT_RATIO: f32 = 0.4;
const LIST_BOTTOM: f32 = 7.0;
/// Padding inside a row.
const ENTRY: f32 = 6.0;
/// Files listed for a query at most.
const FILES: usize = 50;
const DESCRIPTION_SIZE: f32 = input::FONT_SIZE * 0.9;
const KEY_SIZE: f32 = 11.0;

const BACKGROUND: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
const BORDER: Color32 = Color32::from_rgb(0x31, 0x31, 0x31);
const SELECTED: Color32 = Color32::from_rgb(0x04, 0x39, 0x5e);
const SELECTED_FOREGROUND: Color32 = Color32::WHITE;
const HOVER: Color32 = Color32::from_rgb(0x2a, 0x2d, 0x2e);
const HIGHLIGHT: Color32 = Color32::from_rgb(0x2a, 0xaa, 0xff);
const DESCRIPTION: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);
/// `descriptionForeground` at VS Code's 70 % opacity.
const DIM_DESCRIPTION: Color32 = Color32::from_rgba_unmultiplied_const(0x9d, 0x9d, 0x9d, 179);
const KEY_BACKGROUND: Color32 = Color32::from_rgba_unmultiplied_const(128, 128, 128, 43);
const KEY_BORDER: Color32 = Color32::from_rgba_unmultiplied_const(51, 51, 51, 153);
const KEY_BOTTOM: Color32 = Color32::from_rgba_unmultiplied_const(68, 68, 68, 153);

/// Quick open's hint and name.
pub(crate) const FILES_HINT: &str = "Search files by name (append : to go to line)";
/// The command palette's name.
pub(crate) const COMMANDS_LABEL: &str = "Type the name of a command to run.";
/// Go to line's name.
pub(crate) const LINE_LABEL: &str =
    "Go to line. Type a line number, optionally followed by colon and column number.";

const PANEL: Panel = Panel {
    radius: 12,
    background: BACKGROUND,
    border: BORDER,
};

fn widget_id() -> Id {
    Id::new("rustode-quick-input")
}

/// The text field's id.
pub(crate) fn input_id() -> Id {
    Id::new("rustode-quick-input-field")
}

fn list_id() -> Id {
    Id::new("rustode-quick-input-list")
}

fn option_id(index: usize) -> Id {
    Id::new(("rustode-quick-input-option", index))
}

/// A row the user can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Accepted {
    /// Open or activate the file's tab.
    File(PathBuf),
    /// Run the command with this registry id.
    Command(&'static str),
    /// Go to a 1-based line and column.
    Line { line: usize, column: Option<usize> },
}

/// How the box closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Close {
    /// Enter or a click on a row.
    Accept(Accepted),
    /// Escape: focus goes back to what had it.
    Cancel,
    /// Something else took the focus.
    Blur,
}

/// One row of the list.
#[derive(Debug, Clone)]
enum Item {
    File(FileMatch),
    Command(CommandMatch),
    Line {
        line: usize,
        column: Option<usize>,
        label: String,
    },
    /// A prompt or "no results"; Enter does nothing on it.
    Hint(String),
}

impl Item {
    fn label(&self) -> &str {
        match self {
            Self::File(file) => &file.label,
            Self::Command(found) => found.command.label,
            Self::Line { label, .. } | Self::Hint(label) => label,
        }
    }

    fn accepted(&self) -> Option<Accepted> {
        Some(match self {
            Self::File(file) => Accepted::File(file.path.clone()),
            Self::Command(found) => Accepted::Command(found.command.id),
            &Self::Line { line, column, .. } => Accepted::Line { line, column },
            Self::Hint(_) => return None,
        })
    }
}

/// A navigation key in the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nav {
    Up,
    Down,
    PageUp,
    PageDown,
    Accept,
    Cancel,
    /// Tab: the box keeps the focus.
    Stay,
}

fn nav_key(event: &Event) -> Option<Nav> {
    let Event::Key {
        key,
        pressed: true,
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    if modifiers.ctrl || modifiers.command || modifiers.alt {
        return None;
    }
    Some(match key {
        Key::ArrowUp => Nav::Up,
        Key::ArrowDown => Nav::Down,
        Key::PageUp => Nav::PageUp,
        Key::PageDown => Nav::PageDown,
        Key::Enter => Nav::Accept,
        Key::Escape => Nav::Cancel,
        Key::Tab => Nav::Stay,
        _ => return None,
    })
}

/// What the box keeps while it is open.
#[derive(Debug)]
pub(crate) struct QuickInput {
    text: String,
    selected: usize,
    /// The first row in view.
    top: usize,
    /// Where the caret goes when the field takes focus on the next frame.
    caret: Option<Caret>,
    /// Whether the field has had focus; losing it then closes the box.
    armed: bool,
    /// What had focus before the box opened.
    previous: Option<Id>,
    items: Vec<Item>,
    /// The text `items` were listed for.
    listed: Option<String>,
    /// The row clicked on the last frame, run on this one.
    pending: Option<usize>,
}

impl QuickInput {
    /// The box with `prefix` typed in (`""`, `">"` or `":"`).
    pub(crate) fn new(ctx: &Context, prefix: &str) -> Self {
        ctx.request_repaint();
        Self {
            text: prefix.to_owned(),
            selected: 0,
            top: 0,
            caret: Some(Caret::End),
            armed: false,
            previous: ctx.memory(egui::Memory::focused),
            items: Vec::new(),
            listed: None,
            pending: None,
        }
    }

    /// Replaces the text with `prefix`, as opening the box again does.
    pub(crate) fn restart(&mut self, ctx: &Context, prefix: &str) {
        prefix.clone_into(&mut self.text);
        self.selected = 0;
        self.top = 0;
        self.caret = Some(Caret::End);
        ctx.request_repaint();
    }

    /// What had focus before the box opened.
    pub(crate) fn previous(&self) -> Option<Id> {
        self.previous
    }

    /// Ctrl+P in the box: the next row.
    pub(crate) fn select_next(&mut self) {
        self.navigate(Nav::Down, 1);
    }

    /// The rows for the current text, listed again only when it changed.
    fn refresh(&mut self, workspace: &mut Workspace) {
        if self.listed.as_deref() == Some(self.text.as_str()) {
            return;
        }
        self.items = items(workspace, &self.text);
        self.listed = Some(self.text.clone());
        self.selected = 0;
        self.top = 0;
    }

    /// Runs a navigation key; returns how the box closed, if it did.
    fn navigate(&mut self, nav: Nav, page: usize) -> Option<Close> {
        let count = self.items.len().max(1);
        let last = count - 1;
        let page = page.max(1);
        self.selected = match nav {
            Nav::Up => self.selected.checked_sub(1).unwrap_or(last),
            Nav::Down if self.selected >= last => 0,
            Nav::Down => self.selected + 1,
            Nav::PageUp => self.selected.saturating_sub(page),
            Nav::PageDown => (self.selected + page).min(last),
            Nav::Accept => {
                let item = self.items.get(self.selected)?;
                return item.accepted().map(Close::Accept);
            }
            Nav::Cancel => return Some(Close::Cancel),
            Nav::Stay => self.selected,
        };
        None
    }

    /// Keeps the selected row among the `rows` in view.
    fn keep_in_view(&mut self, rows: usize) {
        if self.selected < self.top {
            self.top = self.selected;
        } else if rows > 0 && self.selected >= self.top + rows {
            self.top = self.selected + 1 - rows;
        }
        self.top = self.top.min(self.items.len().saturating_sub(rows));
    }

    /// What the box searches, for screen readers.
    fn name(&self) -> &'static str {
        match parse_quick_query(&self.text) {
            QuickQuery::Files(_) => FILES_HINT,
            QuickQuery::Commands(_) => COMMANDS_LABEL,
            QuickQuery::Line { .. } | QuickQuery::NoLine => LINE_LABEL,
        }
    }
}

/// The rows for `text`, from the workspace.
fn items(workspace: &mut Workspace, text: &str) -> Vec<Item> {
    let lines = workspace
        .active_document()
        .and_then(|id| workspace.document(id))
        .map(Document::line_count);
    let hint = |text: &str| vec![Item::Hint(text.to_owned())];
    match parse_quick_query(text) {
        QuickQuery::Files(query) => {
            let files = workspace.quick_open(query, FILES);
            if files.is_empty() {
                hint("No matching results")
            } else {
                files.into_iter().map(Item::File).collect()
            }
        }
        QuickQuery::Commands(query) => {
            let commands = workspace.commands(query);
            if commands.is_empty() {
                hint("No matching commands")
            } else {
                commands.into_iter().map(Item::Command).collect()
            }
        }
        _ if lines.is_none() => hint("Open a text editor first to go to a line or an offset."),
        QuickQuery::Line { line, column } => {
            let shown = line.min(lines.unwrap_or(1));
            let label = match column {
                Some(column) => format!("Press 'Enter' to go to line {shown} at column {column}."),
                None => format!(
                    "Press 'Enter' to go to line {shown} or enter colon : to add a column number."
                ),
            };
            vec![Item::Line {
                line,
                column,
                label,
            }]
        }
        QuickQuery::NoLine => hint(&format!(
            "Type a line number to go to (from 1 to {}).",
            lines.unwrap_or(1)
        )),
    }
}

/// Rows of the list that fit in `window`.
fn rows_fitting(window: Rect) -> usize {
    let height = (window.height() * HEIGHT_RATIO).min(ROW_HEIGHT * float(MAX_ROWS));
    count((height / ROW_HEIGHT).floor()).max(1)
}

/// A whole, non-negative float as a count.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // float to int casts saturate
fn count(value: f32) -> usize {
    value.max(0.0) as usize
}

/// A count of rows (at most 20) as a float.
#[allow(clippy::cast_precision_loss)] // exact below 2^24
fn float(count: usize) -> f32 {
    count as f32
}

/// Where the box goes: over `group`, the tabs and the editor, as wide as
/// VS Code makes it in `window`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Place {
    pub(crate) group: Rect,
    pub(crate) window: Rect,
}

impl Place {
    /// The box's rectangle with `rows` rows: centered over the group, kept
    /// inside the window.
    fn rect(self, rows: usize) -> Rect {
        let width = (self.window.width() * WIDTH_RATIO).min(MAX_WIDTH);
        let height = HEADER + float(rows) * ROW_HEIGHT + LIST_BOTTOM;
        let left = (self.group.center().x - width / 2.0)
            .min(self.window.right() - width)
            .max(self.window.left());
        Rect::from_min_size(pos2(left, self.group.top() + TOP), vec2(width, height))
    }
}

/// Shows the box over the top of `group` (the tabs and the editor), running
/// its keys first. Returns how it closed, if it did.
pub(crate) fn show(
    ui: &mut Ui,
    workspace: &mut Workspace,
    quick: &mut QuickInput,
    place: Place,
) -> Option<Close> {
    let ctx = &ui.ctx().clone();
    if let Some(accepted) = quick
        .pending
        .take()
        .and_then(|index| quick.items.get(index)?.accepted())
    {
        return Some(Close::Accept(accepted));
    }
    quick.refresh(workspace);
    let fitting = rows_fitting(place.window);
    if ctx.memory(|memory| memory.has_focus(input_id())) {
        for nav in input::take_keys(ctx, nav_key) {
            if let Some(close) = quick.navigate(nav, fitting) {
                return Some(close);
            }
        }
    }
    let rows = quick.items.len().min(fitting);
    let rect = place.rect(rows);
    // Shown after the find widget, so above it.
    let layer = Id::new("rustode-quick-input-layer");
    let (clicked, focused, panel) = input::overlay(ui, layer, rect, |ui| {
        body(ui, workspace, quick, place, rect)
    });
    if let Some(index) = clicked {
        // Run it on the next frame, which has no click: a click in the
        // same frame would take the focus from what the row gives it to.
        quick.selected = index;
        quick.pending = Some(index);
        ctx.request_repaint();
    }
    if focused {
        quick.armed = true;
    } else if quick.armed {
        if !input::clicked_inside(ctx, panel) {
            return Some(Close::Blur);
        }
        ctx.memory_mut(|memory| memory.request_focus(input_id()));
    }
    None
}

/// The panel, the field and the list. Returns the row clicked, if any,
/// whether the field has focus, and the panel's rectangle.
fn body(
    ui: &mut Ui,
    workspace: &mut Workspace,
    quick: &mut QuickInput,
    place: Place,
    rect: Rect,
) -> (Option<usize>, bool, Rect) {
    // Not focusable: a click on the box leaves the focus in its field.
    ui.interact(rect, widget_id(), Sense::CLICK);
    let slots = [ui.painter().add(Shape::Noop), ui.painter().add(Shape::Noop)];
    let field = Rect::from_min_size(
        pos2(rect.left() + PADDING, rect.top() + PADDING),
        vec2(rect.width() - 2.0 * PADDING, INPUT_HEIGHT),
    );
    let caret = quick.caret.take();
    let response = input::text_field(
        ui,
        &mut quick.text,
        &Field {
            id: input_id(),
            rect: field,
            text: field,
            hint: FILES_HINT,
            radius: 6,
            error: None,
        },
        caret,
    );
    if response.changed() {
        quick.refresh(workspace);
        ui.ctx().request_repaint();
    }
    let rows = quick.items.len().min(rows_fitting(place.window));
    quick.keep_in_view(rows);
    let panel = place.rect(rows);
    for (slot, shape) in slots.into_iter().zip(PANEL.shapes(panel)) {
        ui.painter().set(slot, shape);
    }
    let list = Rect::from_min_size(
        pos2(rect.left() + PADDING, rect.top() + HEADER),
        vec2(rect.width() - 2.0 * PADDING, float(rows) * ROW_HEIGHT),
    );
    let clicked = list_rows(ui, quick, list, rows);
    expose_field(ui, quick, rows);
    (clicked, response.has_focus(), panel)
}

/// The field as a combo box that controls the list.
fn expose_field(ui: &Ui, quick: &QuickInput, rows: usize) {
    let shown = (quick.top..quick.top + rows).contains(&quick.selected);
    ui.ctx().accesskit_node_builder(input_id(), |node| {
        node.set_role(accesskit::Role::EditableComboBox);
        node.set_label(quick.name());
        node.set_controls(vec![list_id().accesskit_id()]);
        node.set_has_popup(accesskit::HasPopup::Listbox);
        node.set_expanded(true);
        if shown {
            node.set_active_descendant(option_id(quick.selected).accesskit_id());
        }
    });
}

/// The rows in view; returns the one clicked, if any.
fn list_rows(ui: &mut Ui, quick: &QuickInput, list: Rect, rows: usize) -> Option<usize> {
    ui.interact(list, list_id(), Sense::hover());
    let total = quick.items.len();
    ui.ctx().accesskit_node_builder(list_id(), |node| {
        node.set_role(accesskit::Role::ListBox);
        node.set_label(quick.name());
        node.set_size_of_set(total);
        node.set_bounds(a11y::bounds(list));
    });
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(list)
            .id_salt("rustode-quick-input-rows")
            .accessibility_parent(list_id()),
        |ui| {
            let mut clicked = None;
            let shown = quick.items.iter().enumerate().skip(quick.top).take(rows);
            for (row, (index, item)) in shown.enumerate() {
                let place = Rect::from_min_size(
                    pos2(list.left(), list.top() + float(row) * ROW_HEIGHT),
                    vec2(list.width(), ROW_HEIGHT),
                );
                let selected = index == quick.selected;
                let response = ui.interact(place, option_id(index), Sense::CLICK);
                expose_option(ui, item, index, place, selected);
                paint_row(ui, item, place, selected, response.hovered());
                if response.clicked() {
                    clicked = Some(index);
                }
            }
            clicked
        },
    )
    .inner
}

fn expose_option(ui: &Ui, item: &Item, index: usize, place: Rect, selected: bool) {
    ui.ctx().accesskit_node_builder(option_id(index), |node| {
        node.set_role(accesskit::Role::ListBoxOption);
        node.set_label(item.label());
        match item {
            Item::File(file) if !file.description.is_empty() => {
                node.set_description(file.description.clone());
            }
            Item::Command(CommandMatch {
                keybinding: Some(keybinding),
                ..
            }) => node.set_keyboard_shortcut(keybinding.clone()),
            _ => {}
        }
        node.set_selected(selected);
        node.set_position_in_set(index + 1);
        node.set_bounds(a11y::bounds(place));
        node.add_action(accesskit::Action::Click);
    });
}

/// `text` with the chars at `positions` in the highlight color.
fn highlighted(text: &str, positions: &[usize], size: f32, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut run = String::new();
    let mut run_color = color;
    for (index, ch) in text.chars().enumerate() {
        let wanted = if positions.contains(&index) {
            HIGHLIGHT
        } else {
            color
        };
        if wanted != run_color && !run.is_empty() {
            append(&mut job, &std::mem::take(&mut run), size, run_color);
        }
        run_color = wanted;
        run.push(ch);
    }
    append(&mut job, &run, size, run_color);
    job
}

fn append(job: &mut LayoutJob, text: &str, size: f32, color: Color32) {
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: FontId::proportional(size),
            color,
            ..TextFormat::default()
        },
    );
}

fn paint_row(ui: &Ui, item: &Item, place: Rect, selected: bool, hovered: bool) {
    let painter = ui.painter_at(place);
    if selected {
        painter.rect_filled(place, CornerRadius::same(3), SELECTED);
    } else if hovered {
        painter.rect_filled(place, CornerRadius::same(3), HOVER);
    }
    let (foreground, description) = if selected {
        (SELECTED_FOREGROUND, DESCRIPTION)
    } else {
        (FOREGROUND, DIM_DESCRIPTION)
    };
    let (label, detail): (LayoutJob, Option<LayoutJob>) = match item {
        Item::File(file) => (
            highlighted(&file.label, &file.score.label, input::FONT_SIZE, foreground),
            Some(highlighted(
                &file.description,
                &file.score.description,
                DESCRIPTION_SIZE,
                description,
            )),
        ),
        Item::Command(found) => (
            highlighted(
                found.command.label,
                &found.positions,
                input::FONT_SIZE,
                foreground,
            ),
            None,
        ),
        Item::Line { label, .. } => (highlighted(label, &[], input::FONT_SIZE, foreground), None),
        Item::Hint(hint) => (highlighted(hint, &[], input::FONT_SIZE, description), None),
    };
    let label = painter.layout_job(label);
    let left = place.left() + ENTRY;
    let center = place.center().y;
    painter.galley(
        pos2(left, center - label.size().y / 2.0),
        Arc::clone(&label),
        foreground,
    );
    if let Some(detail) = detail {
        let detail = painter.layout_job(detail);
        let x = left + label.size().x + input::FONT_SIZE / 2.0;
        painter.galley(pos2(x, center - detail.size().y / 2.0), detail, description);
    }
    if let Item::Command(CommandMatch {
        keybinding: Some(keybinding),
        ..
    }) = item
    {
        paint_keybinding(&painter, keybinding, place);
    }
}

/// A keybinding as keys, right-aligned in `place`: `Ctrl` `+` `Shift` `+` `P`.
fn paint_keybinding(painter: &egui::Painter, keybinding: &str, place: Rect) {
    let font = FontId::proportional(KEY_SIZE);
    let mut parts: Vec<(Arc<Galley>, bool)> = Vec::new();
    for (chord_index, chord) in keybinding.split(' ').enumerate() {
        if chord_index > 0 {
            parts.push((
                painter.layout_no_wrap(" ".to_owned(), font.clone(), FOREGROUND),
                false,
            ));
        }
        for (key_index, key) in chord.split('+').enumerate() {
            if key_index > 0 {
                parts.push((
                    painter.layout_no_wrap("+".to_owned(), font.clone(), FOREGROUND),
                    false,
                ));
            }
            parts.push((
                painter.layout_no_wrap(key.to_owned(), font.clone(), FOREGROUND),
                true,
            ));
        }
    }
    let width = |(galley, key): &(Arc<Galley>, bool)| {
        galley.size().x + if *key { 2.0 * 5.0 + 2.0 * 2.0 } else { 0.0 }
    };
    let total: f32 = parts.iter().map(width).sum();
    let mut x = place.right() - ENTRY - total;
    let center = place.center().y;
    for part in &parts {
        let (galley, key) = part;
        let size = width(part);
        if *key {
            let rect = Rect::from_min_size(pos2(x + 2.0, center - 8.0), vec2(size - 4.0, 16.0));
            painter.rect(
                rect,
                CornerRadius::same(3),
                KEY_BACKGROUND,
                Stroke::new(1.0, KEY_BORDER),
                StrokeKind::Inside,
            );
            painter.hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                Stroke::new(1.0, KEY_BOTTOM),
            );
        }
        let pos = pos2(
            x + (size - galley.size().x) / 2.0,
            center - galley.size().y / 2.0,
        );
        painter.galley(pos, Arc::clone(galley), FOREGROUND);
        x += size;
    }
}
