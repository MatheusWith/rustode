//! The find widget at the top right of the editor (Ctrl+F, Ctrl+H), as the
//! TUI's (`crates/tui/src/find.rs`) and VS Code's. Every search runs in the
//! workspace (`find_next`, `find_previous`, `find_all`, `replace_one`,
//! `replace_all`); the widget keeps its fields and toggles, kept when it
//! closes so F3 repeats the last search, and the matches of the last
//! search, found again only when the text or the query changes.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//!
//! - a dialog named "Find / Replace" whose fields are text inputs named and
//!   hinted "Find" and "Replace"; Previous Match, Next Match, Close,
//!   Replace, Replace All and Toggle Replace buttons, the last one
//!   expanded while the replace field shows
//!   (`src/vs/editor/contrib/find/browser/findWidget.ts:65-79`,
//!   `:1000-1001`, `:1169-1170`, `:1266-1269`, `:1471-1472`); the Match
//!   Case, Match Whole Word and Use Regular Expression toggles, buttons
//!   that expose whether they are on
//!   (`src/vs/base/browser/ui/findinput/findInputToggles.ts:58-60`); the
//!   buttons are enabled as VS Code enables them (`findWidget.ts`,
//!   `_updateButtons`);
//! - the count: "1 of 3", "?" for the position when the selection is not
//!   on a match, a `+` at the 19,999 limit, "No results", in the error
//!   color when the find field is not empty (`:459-486`, `:428-431`;
//!   `findWidget.css:222-224`);
//! - 419 points wide less the 25-point Find in Selection button rustode
//!   does not have, 34 points high and 64 with the replace field, 4 points
//!   below the editor's top (`findWidget.ts:81-86`, `:810-831`;
//!   `findWidget.css:7-23`), 28 points from its right, the two scrollbar
//!   widths VS Code leaves
//!   (`src/vs/editor/browser/viewParts/overlayWidgets/overlayWidgets.ts:182`,
//!   `src/vs/editor/common/config/editorOptions.ts:4317`), and never wider
//!   than the editor less 43 points (`findWidget.ts:796`); fields of
//!   13-point text 26 points high, the count in 12-point text, 22-point
//!   buttons 3 points apart, 20-point toggles
//!   (`findWidget.css:58-67`, `:115-140`;
//!   `src/vs/base/browser/ui/toggle/toggle.css:6-15`);
//! - the keys of `src/vs/editor/contrib/find/browser/findController.ts`:
//!   Enter and Shift+Enter in the find field for the next and previous
//!   match (`:783-809`), Enter in the replace field replaces one, and
//!   Ctrl+Enter there or Ctrl+Alt+Enter replace all (`:1154-1195`); Escape
//!   closes, from the fields or the editor (`:1078`); Alt+C, Alt+W and
//!   Alt+R toggle the options, from the fields or the editor
//!   (`findModel.ts:39-50`); Ctrl+H focuses the replace field when the find
//!   field had focus or the selection seeded the search, else the find
//!   field (`:1031-1047`); Tab and Shift+Tab move between the two fields;
//! - Dark Modern's widget, border, option and error colors
//!   (`extensions/theme-defaults/themes/dark_modern.json:46-50`, `:55-56`,
//!   `:133`), and the color registry's defaults for the rest
//!   (`src/vs/platform/theme/common/colors/inputColors.ts:44-46`,
//!   `:79-89`; `baseColors.ts:21-22`; `editorColors.ts:350-351`).
//!
//! Not done yet: Find in Selection and Preserve Case (the workspace has no
//! search scope or case-preserving replace), the narrow layouts VS Code
//! switches to in small editors, resizing, and history.

use crate::a11y;
use crate::input::{self, Caret, FOREGROUND, Field};
use buttons::{Button, Icon, button};
use eframe::egui::{
    Color32, Context, CornerRadius, Event, FontId, Id, Key, Label, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2, accesskit, pos2, vec2,
};

mod buttons;

use rustode_workspace::{
    Document, DocumentId, FindOptions, FindQuery, MATCHES_LIMIT, Range, Workspace,
};

const WIDTH: f32 = 419.0 - SLOT;
const MARGIN: f32 = 4.0;
const INPUT_HEIGHT: f32 = 26.0;
/// From the widget's left edge to the fields: the Toggle Replace button.
const LEFT: f32 = 26.0;
const TOGGLE_REPLACE_WIDTH: f32 = 18.0;
const COUNT_WIDTH: f32 = 69.0;
const BUTTON: f32 = 22.0;
/// A button and the space before it.
const SLOT: f32 = 25.0;
const TOGGLE: f32 = 20.0;
/// The three toggles inside the find field, with their spacing.
const TOGGLES: f32 = 3.0 * (TOGGLE + 2.0) + 2.0;
const TOP: f32 = 4.0;
const RIGHT: f32 = 28.0;
const MIN_LEFT: f32 = 15.0;
const COUNT_SIZE: f32 = 12.0;

const BACKGROUND: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
const BORDER: Color32 = Color32::from_rgb(0x31, 0x31, 0x31);
const ERROR: Color32 = Color32::from_rgb(0xf8, 0x51, 0x49);
const ERROR_BACKGROUND: Color32 = Color32::from_rgb(0x5a, 0x1d, 0x1d);
const ERROR_BORDER: Color32 = Color32::from_rgb(0xbe, 0x11, 0x00);
const DISABLED: Color32 = Color32::from_rgba_unmultiplied_const(0xcc, 0xcc, 0xcc, 0x80);
const HOVER: Color32 = Color32::from_rgba_unmultiplied_const(0x5a, 0x5d, 0x5e, 0x50);
const OPTION_BACKGROUND: Color32 = Color32::from_rgba_unmultiplied_const(0x24, 0x89, 0xdb, 0x82);
const OPTION_BORDER: Color32 = Color32::from_rgb(0x24, 0x88, 0xdb);

/// The dialog's name for screen readers.
pub(crate) const DIALOG_LABEL: &str = "Find / Replace";
/// The count when nothing matches.
pub(crate) const NO_RESULTS: &str = "No results";

fn dialog_id() -> Id {
    Id::new("rustode-find")
}

/// The find field's id.
pub(crate) fn find_id() -> Id {
    Id::new("rustode-find-input")
}

/// The replace field's id.
pub(crate) fn replace_id() -> Id {
    Id::new("rustode-replace-input")
}

/// One of the two fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Find,
    Replace,
}

impl Part {
    fn id(self) -> Id {
        match self {
            Self::Find => find_id(),
            Self::Replace => replace_id(),
        }
    }
}

/// A find option and its toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Toggle {
    Case,
    Word,
    Regex,
}

impl Toggle {
    const ALL: [Self; 3] = [Self::Case, Self::Word, Self::Regex];

    fn label(self) -> &'static str {
        match self {
            Self::Case => "Match Case",
            Self::Word => "Match Whole Word",
            Self::Regex => "Use Regular Expression",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Self::Case => "Aa",
            Self::Word => "ab",
            Self::Regex => ".*",
        }
    }

    fn of(key: Key) -> Option<Self> {
        Some(match key {
            Key::C => Self::Case,
            Key::W => Self::Word,
            Key::R => Self::Regex,
            _ => return None,
        })
    }

    fn flag(self, options: &mut FindOptions) -> &mut bool {
        match self {
            Self::Case => &mut options.match_case,
            Self::Word => &mut options.whole_word,
            Self::Regex => &mut options.regex,
        }
    }
}

/// What a key does in the widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Close,
    Next,
    Previous,
    ReplaceOne,
    ReplaceAll,
    OtherField,
    Toggle(Toggle),
}

/// The matches of the last search, and what they were found for.
#[derive(Debug)]
struct Matches {
    document: DocumentId,
    version: u64,
    pattern: String,
    options: FindOptions,
    /// The matches, or why the pattern is not a valid regex.
    found: Result<Vec<Range>, String>,
}

/// What the widget keeps between frames.
#[derive(Debug, Default)]
pub(crate) struct Find {
    /// Whether the widget is shown.
    pub(crate) shown: bool,
    query: String,
    replacement: String,
    options: FindOptions,
    /// Whether the replace field is shown.
    replace_shown: bool,
    /// Where typing searches from: the selection's start when it opened.
    origin: usize,
    /// The field that takes focus on the next frame, with its text selected.
    focus: Option<Part>,
    matches: Option<Matches>,
}

/// What a frame of the widget did.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// Whether anything changed.
    pub(crate) changed: bool,
    /// Whether the selection moved, so the editor should scroll to it.
    pub(crate) reveal: bool,
    /// For the status bar.
    pub(crate) message: Option<String>,
    /// The widget closed: the editor takes focus.
    pub(crate) focus_editor: bool,
}

impl Outcome {
    fn report(&mut self, result: Result<String, String>) {
        match result {
            Ok(message) if message.is_empty() => {}
            Ok(message) | Err(message) => self.message = Some(message),
        }
    }
}

/// The selected text when the selection sits on one line, to search for.
fn seed(document: &Document, selection: Range) -> Option<String> {
    let start = document.position_of(selection.start);
    let end = document.position_of(selection.end);
    (start.line == end.line && start.column < end.column).then(|| {
        document
            .line(start.line)
            .chars()
            .skip(start.column)
            .take(end.column - start.column)
            .collect()
    })
}

impl Find {
    /// Ctrl+F, or Ctrl+H with `replace`: shows the widget, seeded with a
    /// selection on one line unless the find field has focus, and searches
    /// from the selection's start.
    pub(crate) fn open(&mut self, ctx: &Context, workspace: &mut Workspace, replace: bool) -> bool {
        let Some(document) = workspace
            .active_document()
            .and_then(|id| workspace.document(id))
        else {
            return false;
        };
        let in_find = ctx.memory(|memory| memory.has_focus(find_id()));
        let selection = document.primary_selection().range();
        let seeded = if in_find {
            None
        } else {
            seed(document, selection)
        };
        let focus_replace = replace && (in_find || seeded.is_some());
        if let Some(seeded) = seeded {
            self.query = seeded;
        }
        self.origin = selection.start;
        self.shown = true;
        self.replace_shown |= replace;
        self.focus = Some(if focus_replace {
            Part::Replace
        } else {
            Part::Find
        });
        ctx.request_repaint();
        self.search_from_origin(workspace)
    }

    fn query(&self) -> Result<FindQuery, String> {
        FindQuery::new(&self.query, self.options).map_err(|err| err.to_string())
    }

    /// Selects the first match from where the widget opened; returns
    /// whether the selection moved.
    fn search_from_origin(&mut self, workspace: &mut Workspace) -> bool {
        let (Some(id), Ok(query)) = (workspace.active_document(), self.query()) else {
            return false;
        };
        if query.pattern().is_empty() {
            return false;
        }
        let _ = workspace.set_cursor(id, self.origin);
        let _ = workspace.find_next(id, &query);
        true
    }

    /// F3 / Enter and Shift+F3 / Shift+Enter: the next or previous match,
    /// wrapping around; also with the widget closed, for the last search.
    pub(crate) fn step(&self, workspace: &mut Workspace, forward: bool) -> Result<bool, String> {
        let Some(id) = workspace.active_document() else {
            return Ok(false);
        };
        let query = self.query()?;
        let found = if forward {
            workspace.find_next(id, &query)
        } else {
            workspace.find_previous(id, &query)
        };
        found
            .map(|found| found.is_some())
            .map_err(|err| err.to_string())
    }

    fn replace_one(&self, workspace: &mut Workspace) -> Result<String, String> {
        let Some(id) = workspace.active_document() else {
            return Ok(String::new());
        };
        let query = self.query()?;
        workspace
            .replace_one(id, &query, &self.replacement)
            .map(|_| String::new())
            .map_err(|err| err.to_string())
    }

    fn replace_all(&self, workspace: &mut Workspace) -> Result<String, String> {
        let Some(id) = workspace.active_document() else {
            return Ok(String::new());
        };
        let query = self.query()?;
        workspace
            .replace_all(id, &query, &self.replacement)
            .map(|count| format!("Replaced {count} occurrence(s)"))
            .map_err(|err| err.to_string())
    }

    /// Finds the matches again if the text or the query changed.
    fn refresh(&mut self, workspace: &Workspace) {
        let Some(id) = workspace.active_document() else {
            self.matches = None;
            return;
        };
        let version = workspace.document(id).map_or(0, Document::version);
        let current = self.matches.as_ref().is_some_and(|m| {
            m.document == id
                && m.version == version
                && m.pattern == self.query
                && m.options == self.options
        });
        if current {
            return;
        }
        let found = self
            .query()
            .map(|query| workspace.find_all(id, &query).unwrap_or_default());
        self.matches = Some(Matches {
            document: id,
            version,
            pattern: self.query.clone(),
            options: self.options,
            found,
        });
    }

    /// The matches to highlight in the active document while the widget is
    /// shown, sorted.
    pub(crate) fn highlights(&mut self, workspace: &Workspace) -> &[Range] {
        if !self.shown {
            return &[];
        }
        self.refresh(workspace);
        match &self.matches {
            Some(Matches {
                found: Ok(found), ..
            }) => found,
            _ => &[],
        }
    }

    /// Whether the text changed since the matches were found, so another
    /// frame must draw them again.
    pub(crate) fn is_stale(&self, workspace: &Workspace) -> bool {
        let active = workspace
            .active_document()
            .and_then(|id| workspace.document(id).map(|doc| (id, doc.version())));
        self.shown
            && self
                .matches
                .as_ref()
                .is_some_and(|m| Some((m.document, m.version)) != active)
    }

    /// The count, and whether it reads as an error.
    fn count(&mut self, workspace: &Workspace) -> (String, bool) {
        self.refresh(workspace);
        let empty = self.query.is_empty();
        let Some(Matches {
            found: Ok(found),
            document,
            ..
        }) = &self.matches
        else {
            return (NO_RESULTS.to_owned(), !empty);
        };
        if found.is_empty() {
            return (NO_RESULTS.to_owned(), !empty);
        }
        let total = if found.len() >= MATCHES_LIMIT {
            format!("{MATCHES_LIMIT}+")
        } else {
            found.len().to_string()
        };
        let position = workspace
            .document(*document)
            .map(|doc| doc.primary_selection().range())
            .and_then(|selection| {
                let index = found
                    .binary_search_by_key(&selection.start, |m| m.start)
                    .ok()?;
                (found[index] == selection).then_some(index)
            })
            .map_or_else(|| "?".to_owned(), |index| (index + 1).to_string());
        (format!("{position} of {total}"), false)
    }

    /// Why the pattern is not a valid regex, if it is not.
    fn error(&self) -> Option<&str> {
        self.matches
            .as_ref()
            .and_then(|m| m.found.as_ref().err())
            .map(String::as_str)
    }

    fn apply(&mut self, workspace: &mut Workspace, action: Action, outcome: &mut Outcome) {
        outcome.changed = true;
        match action {
            Action::Close => {
                self.shown = false;
                outcome.focus_editor = true;
            }
            Action::Next | Action::Previous => match self.step(workspace, action == Action::Next) {
                Ok(moved) => outcome.reveal |= moved,
                Err(err) => outcome.message = Some(err),
            },
            Action::ReplaceOne => {
                outcome.report(self.replace_one(workspace));
                outcome.reveal = true;
            }
            Action::ReplaceAll => outcome.report(self.replace_all(workspace)),
            // `show` moves the focus: it knows which field has it.
            Action::OtherField => {}
            Action::Toggle(toggle) => {
                let flag = toggle.flag(&mut self.options);
                *flag = !*flag;
                outcome.reveal |= self.search_from_origin(workspace);
            }
        }
    }
}

/// The widget's key for one event while `part` has focus.
fn field_key(event: &Event, part: Part, replace_shown: bool) -> Option<Action> {
    let Event::Key {
        key,
        pressed: true,
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    let ctrl = modifiers.command || modifiers.ctrl;
    Some(match key {
        Key::Escape => Action::Close,
        Key::Enter if ctrl && (modifiers.alt || part == Part::Replace) => Action::ReplaceAll,
        Key::Enter if ctrl || modifiers.alt => return None,
        Key::Enter if part == Part::Replace => Action::ReplaceOne,
        Key::Enter if modifiers.shift => Action::Previous,
        Key::Enter => Action::Next,
        Key::Tab if replace_shown && !ctrl && !modifiers.alt => Action::OtherField,
        _ if modifiers.alt && !ctrl => Action::Toggle(Toggle::of(*key)?),
        _ => return None,
    })
}

/// A widget key that also works while the editor has focus: Escape,
/// Alt+C, Alt+W, Alt+R.
fn editor_key(event: &Event) -> Option<Action> {
    let Event::Key {
        key,
        pressed: true,
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    match key {
        Key::Escape if !modifiers.any() => Some(Action::Close),
        _ if modifiers.alt && !modifiers.ctrl && !modifiers.command && !modifiers.shift => {
            Toggle::of(*key).map(Action::Toggle)
        }
        _ => None,
    }
}

/// Runs the widget's keys taken out of this frame's input.
fn run_keys(
    ctx: &Context,
    workspace: &mut Workspace,
    find: &mut Find,
    actions: &[Action],
) -> Outcome {
    let mut outcome = Outcome::default();
    if actions
        .iter()
        .any(|action| matches!(action, Action::Toggle(_)))
    {
        input::drop_alt_text(ctx, &['c', 'w', 'r']);
    }
    for &action in actions {
        find.apply(workspace, action, &mut outcome);
    }
    outcome
}

/// While the editor has focus and the widget is shown: Escape closes the
/// widget and Alt+C, Alt+W, Alt+R toggle its options.
pub(crate) fn editor_keys(ctx: &Context, workspace: &mut Workspace, find: &mut Find) -> Outcome {
    if !find.shown {
        return Outcome::default();
    }
    let actions = input::take_keys(ctx, editor_key);
    run_keys(ctx, workspace, find, &actions)
}

/// Shows the widget at the top right of `editor`, running its keys first.
pub(crate) fn show(
    ui: &mut Ui,
    workspace: &mut Workspace,
    find: &mut Find,
    editor: Rect,
) -> Outcome {
    if !find.shown || workspace.active_document().is_none() {
        return Outcome::default();
    }
    let ctx = ui.ctx().clone();
    let part = ctx.memory(|memory| {
        if memory.has_focus(find_id()) {
            Some(Part::Find)
        } else if memory.has_focus(replace_id()) {
            Some(Part::Replace)
        } else {
            None
        }
    });
    let mut outcome = match part {
        Some(part) => {
            let replace_shown = find.replace_shown;
            let actions = input::take_keys(&ctx, |event| field_key(event, part, replace_shown));
            if actions.contains(&Action::OtherField) {
                find.focus = Some(match part {
                    Part::Find => Part::Replace,
                    Part::Replace => Part::Find,
                });
            }
            run_keys(&ctx, workspace, find, &actions)
        }
        None => Outcome::default(),
    };
    if !find.shown {
        return outcome;
    }
    let rect = place(editor, find.replace_shown);
    input::overlay(ui, Id::new("rustode-find-layer"), rect, |ui| {
        body(ui, workspace, find, rect, &mut outcome);
    });
    let lost = ctx.memory(|memory| !memory.has_focus(find_id()) && !memory.has_focus(replace_id()));
    if let Some(part) = part.filter(|_| lost && input::clicked_inside(&ctx, rect)) {
        // A click on the widget's buttons keeps the field's focus.
        ctx.memory_mut(|memory| memory.request_focus(part.id()));
    }
    outcome
}

/// The widget's rectangle in `editor`.
fn place(editor: Rect, replace_shown: bool) -> Rect {
    let width = WIDTH.min(editor.width() - RIGHT - MIN_LEFT).max(0.0);
    let rows = if replace_shown { 2.0 } else { 1.0 };
    let height = rows * (MARGIN + INPUT_HEIGHT) + MARGIN;
    Rect::from_min_size(
        pos2(editor.right() - RIGHT - width, editor.top() + TOP),
        vec2(width, height),
    )
}

/// The field's width in a widget `width` points wide.
fn field_width(width: f32) -> f32 {
    (width - LEFT - 3.0 - COUNT_WIDTH - 3.0 * SLOT).max(TOGGLES + 40.0)
}

fn body(
    ui: &mut Ui,
    workspace: &mut Workspace,
    find: &mut Find,
    rect: Rect,
    outcome: &mut Outcome,
) {
    // Not focusable: a click on the widget leaves the focus in its field.
    ui.interact(rect, dialog_id(), Sense::CLICK);
    input::Panel {
        radius: 8,
        background: BACKGROUND,
        border: BORDER,
    }
    .paint(ui, rect);
    ui.ctx().accesskit_node_builder(dialog_id(), |node| {
        node.set_role(accesskit::Role::Dialog);
        node.set_label(DIALOG_LABEL);
        node.set_bounds(a11y::bounds(rect));
    });
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .id_salt("rustode-find-parts")
            .accessibility_parent(dialog_id()),
        |ui| {
            toggle_replace(ui, find, rect, outcome);
            find_row(ui, workspace, find, rect, outcome);
            if find.replace_shown {
                replace_row(ui, workspace, find, rect, outcome);
            }
            if let Some(error) = find.error().map(str::to_owned) {
                error_message(ui, rect, &error);
            }
        },
    );
}

fn toggle_replace(ui: &mut Ui, find: &mut Find, rect: Rect, outcome: &mut Outcome) {
    let place = Rect::from_min_size(rect.min, vec2(TOGGLE_REPLACE_WIDTH, rect.height()));
    let clicked = button(
        ui,
        &Button {
            id: Id::new("rustode-find-toggle-replace"),
            rect: place,
            label: "Toggle Replace",
            icon: Icon::Chevron(find.replace_shown),
            enabled: true,
            toggled: None,
            expanded: Some(find.replace_shown),
        },
    );
    if clicked {
        find.replace_shown = !find.replace_shown;
        find.focus = Some(if find.replace_shown {
            Part::Replace
        } else {
            Part::Find
        });
        outcome.changed = true;
    }
}

/// The find field with its toggles, the count, and the match buttons.
fn find_row(
    ui: &mut Ui,
    workspace: &mut Workspace,
    find: &mut Find,
    rect: Rect,
    outcome: &mut Outcome,
) {
    let field = Rect::from_min_size(
        pos2(rect.left() + LEFT, rect.top() + MARGIN),
        vec2(field_width(rect.width()), INPUT_HEIGHT),
    );
    let text = Rect::from_min_max(field.min, pos2(field.right() - TOGGLES, field.bottom()));
    let caret = (find.focus == Some(Part::Find)).then_some(Caret::SelectAll);
    let error = find.error().is_some().then_some(ERROR_BORDER);
    let response = input::text_field(
        ui,
        &mut find.query,
        &Field {
            id: find_id(),
            rect: field,
            text,
            hint: "Find",
            radius: 6,
            error,
        },
        caret,
    );
    if caret.is_some() {
        find.focus = None;
    }
    label_field(ui, find_id(), "Find");
    if response.changed() {
        outcome.changed = true;
        outcome.reveal |= find.search_from_origin(workspace);
    }
    toggles(ui, workspace, find, field, outcome);
    let (count, failed) = find.count(workspace);
    let count_rect = Rect::from_min_size(
        pos2(field.right() + 3.0, field.top()),
        vec2(COUNT_WIDTH, INPUT_HEIGHT),
    );
    let color = if failed { ERROR } else { FOREGROUND };
    ui.put(
        count_rect,
        Label::new(
            RichText::new(count)
                .font(FontId::proportional(COUNT_SIZE))
                .color(color),
        )
        .selectable(false)
        .truncate(),
    );
    let can_move = !find.query.is_empty() && !failed;
    let buttons = [
        ("Previous Match", Icon::Up, can_move, Action::Previous),
        ("Next Match", Icon::Down, can_move, Action::Next),
        ("Close", Icon::Close, true, Action::Close),
    ];
    let row = Row {
        part: Part::Find,
        field,
        left: count_rect.right(),
        buttons: &buttons,
    };
    row_buttons(ui, workspace, find, &row, outcome);
}

fn replace_row(
    ui: &mut Ui,
    workspace: &mut Workspace,
    find: &mut Find,
    rect: Rect,
    outcome: &mut Outcome,
) {
    let field = Rect::from_min_size(
        pos2(rect.left() + LEFT, rect.top() + 2.0 * MARGIN + INPUT_HEIGHT),
        vec2(field_width(rect.width()), INPUT_HEIGHT),
    );
    let caret = (find.focus == Some(Part::Replace)).then_some(Caret::SelectAll);
    let response = input::text_field(
        ui,
        &mut find.replacement,
        &Field {
            id: replace_id(),
            rect: field,
            text: field,
            hint: "Replace",
            radius: 6,
            error: None,
        },
        caret,
    );
    if caret.is_some() {
        find.focus = None;
    }
    label_field(ui, replace_id(), "Replace");
    outcome.changed |= response.changed();
    let enabled = !find.query.is_empty();
    let buttons = [
        ("Replace", Icon::Replace, enabled, Action::ReplaceOne),
        ("Replace All", Icon::ReplaceAll, enabled, Action::ReplaceAll),
    ];
    let row = Row {
        part: Part::Replace,
        field,
        left: field.right(),
        buttons: &buttons,
    };
    row_buttons(ui, workspace, find, &row, outcome);
}

/// Names a field for screen readers, as VS Code's `aria-label` does.
fn label_field(ui: &Ui, id: Id, label: &str) {
    ui.ctx()
        .accesskit_node_builder(id, |node| node.set_label(label));
}

/// A row's buttons: each with its label, icon, whether it is enabled and
/// what it does.
struct Row<'a> {
    part: Part,
    /// The row's field.
    field: Rect,
    /// Where the buttons start.
    left: f32,
    buttons: &'a [(&'a str, Icon, bool, Action)],
}

/// The buttons of `row`, each running its action; a click leaves the
/// focus in the row's field.
fn row_buttons(
    ui: &mut Ui,
    workspace: &mut Workspace,
    find: &mut Find,
    row: &Row<'_>,
    outcome: &mut Outcome,
) {
    let top = row.field.center().y - BUTTON / 2.0;
    for (index, &(label, icon, enabled, action)) in (0_u8..).zip(row.buttons) {
        let x = row.left + 3.0 + f32::from(index) * SLOT;
        let place = Rect::from_min_size(pos2(x, top), Vec2::splat(BUTTON));
        let clicked = button(
            ui,
            &Button {
                id: Id::new(("rustode-find-button", label)),
                rect: place,
                label,
                icon,
                enabled,
                toggled: None,
                expanded: None,
            },
        );
        if clicked {
            find.apply(workspace, action, outcome);
            if find.focus.is_none() && action != Action::Close {
                find.focus = Some(row.part);
            }
        }
    }
}

/// The three toggles at the right end of the find field.
fn toggles(
    ui: &mut Ui,
    workspace: &mut Workspace,
    find: &mut Find,
    field: Rect,
    outcome: &mut Outcome,
) {
    let top = field.center().y - TOGGLE / 2.0;
    for (index, toggle) in (0_u8..).zip(Toggle::ALL) {
        let right = field.right() - 2.0 - f32::from(2 - index) * (TOGGLE + 2.0);
        let place = Rect::from_min_size(pos2(right - TOGGLE, top), Vec2::splat(TOGGLE));
        let on = *toggle.flag(&mut find.options);
        let clicked = button(
            ui,
            &Button {
                id: Id::new(("rustode-find-toggle", toggle.label())),
                rect: place,
                label: toggle.label(),
                icon: Icon::Text(toggle.glyph()),
                enabled: true,
                toggled: Some(on),
                expanded: None,
            },
        );
        if clicked {
            find.apply(workspace, Action::Toggle(toggle), outcome);
            find.focus = Some(Part::Find);
        }
    }
}

/// The regex error under the find field, as VS Code's input validation
/// message shows it.
fn error_message(ui: &mut Ui, rect: Rect, error: &str) {
    let place = Rect::from_min_size(
        pos2(rect.left() + LEFT, rect.bottom() + 2.0),
        vec2(field_width(rect.width()), 24.0),
    );
    ui.painter().rect(
        place,
        CornerRadius::same(3),
        ERROR_BACKGROUND,
        Stroke::new(1.0, ERROR_BORDER),
        StrokeKind::Inside,
    );
    ui.put(
        place.shrink2(vec2(6.0, 0.0)),
        Label::new(
            RichText::new(error)
                .font(FontId::proportional(COUNT_SIZE))
                .color(FOREGROUND),
        )
        .selectable(false)
        .truncate(),
    );
}
