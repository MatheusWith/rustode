//! The Search view in the sidebar, in place of the explorer, like VS
//! Code's: Ctrl+Shift+F shows it with the search box focused and seeded
//! from a one-line selection (`workbench.action.findInFiles`,
//! `src/vs/workbench/contrib/search/browser/searchActionsFind.ts:203-258`;
//! `searchView.ts:1270-1285`). It holds the search box with the Match
//! Case, Match Whole Word and Use Regular Expression toggles (Alt+C, Alt+W,
//! Alt+R, `searchActionsNav.ts:80-135`; titles from
//! `src/vs/base/browser/ui/findinput/findInputToggles.ts`), the "files to
//! include" and "files to exclude" boxes with VS Code's placeholders
//! (`searchView.ts`, `placeholder.includes` and `placeholder.excludes`),
//! the result count, and the results: each file with its folder and match
//! count after a chevron, then its matches with the matched text
//! highlighted, as VS Code's
//! list view shows them (`search.defaultViewMode: list`,
//! `search.contribution.ts:195-204`). Colors are Dark Modern's
//! (`extensions/theme-defaults/themes/dark_modern.json`).
//!
//! Enter in a box searches; Escape cancels a running search
//! (`search.action.cancel`, `searchActionsTopBar.ts:42-62`), else gives
//! focus back to the editor. A click on a file shows or hides its
//! matches; a click on a match opens it in a tab and focuses the editor.
//! Every widget is an egui widget, so each is in the AccessKit tree: the
//! boxes are text inputs, the toggles are toggle buttons, the files and
//! matches are buttons. All work goes through the workspace, in the
//! background.
//!
//! Replace (Ctrl+Shift+H, `workbench.action.replaceInFiles`,
//! `searchActionsNav.ts:396-413`) adds a replace box under the search box
//! and, on each result, **Replace** and **Dismiss** buttons
//! (`search.action.replace`, `search.action.replaceAllInFile` for a file,
//! `search.action.remove`; `searchActionsRemoveReplace.ts:59-230`). A match
//! shows its text struck through and the text that replaces it next to it,
//! where VS Code opens a diff editor per file (a gap). **Replace All**
//! asks to confirm first (`searchView.ts:833-837`); the work runs a few
//! files per frame and the status line counts them.

use eframe::egui::{
    self, Align, Button, Color32, FontId, Id, Key, Label, Layout, Modifiers, RichText, ScrollArea,
    Sense, Stroke, TextEdit, TextFormat, Ui, Vec2, text::LayoutJob,
};
use rustode_workspace::{FindOptions, FindQuery, ReplaceScope, SearchQuery, SearchRow, Workspace};

const BACKGROUND: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const FOREGROUND: Color32 = Color32::from_rgb(0xcc, 0xcc, 0xcc);
const DESCRIPTION: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);
const INPUT: Color32 = Color32::from_rgb(0x31, 0x31, 0x31);
/// `editor.findMatchHighlightBackground`, as the search view highlights
/// matches.
const MATCH: Color32 = Color32::from_rgba_premultiplied(0x4e, 0x3b, 0x1a, 0x55);
/// The text a replace takes out, and the text it puts in: close to the
/// diff editor's `removedTextBackground` and `insertedTextBackground`.
const REMOVED: Color32 = Color32::from_rgba_premultiplied(0x4d, 0x10, 0x10, 0x55);
const ADDED: Color32 = Color32::from_rgba_premultiplied(0x1d, 0x2f, 0x0f, 0x55);
const FONT_SIZE: f32 = 13.0;
/// The height of one result row.
const ROW_HEIGHT: f32 = 22.0;
/// How far a match row is indented under its file.
const MATCH_INDENT: f32 = 22.0;

/// What the panel keeps between frames.
#[derive(Debug, Default)]
pub(crate) struct Panel {
    /// Whether the sidebar shows this view.
    pub shown: bool,
    query: String,
    include: String,
    exclude: String,
    options: FindOptions,
    /// Focus the search box on the next frame.
    focus_query: bool,
    replacement: String,
    replace_shown: bool,
    /// The question of a Replace All that waits for its answer.
    confirm: Option<String>,
    /// The result last clicked, which the replace commands act on.
    selected: Option<SearchRow>,
    /// Why the last search could not start.
    error: Option<String>,
}

/// What a replace command does to the selected result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// `search.action.replace`: the match, or the file for a file.
    Replace,
    /// `search.action.replaceAllInFile`.
    ReplaceFile,
    /// `search.action.remove`.
    Remove,
    /// `rustode.search.undoReplace`.
    Undo,
}

/// What a frame of the panel asks the window to do.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    /// A message for the status bar.
    pub message: Option<String>,
    /// Give focus back to the editor.
    pub focus_editor: bool,
}

fn query_id() -> Id {
    Id::new("rustode-search-query")
}

fn include_id() -> Id {
    Id::new("rustode-search-include")
}

fn replace_id() -> Id {
    Id::new("rustode-search-replace")
}

fn exclude_id() -> Id {
    Id::new("rustode-search-exclude")
}

/// Whether one of the panel's boxes has keyboard focus.
pub(crate) fn has_focus(ctx: &egui::Context) -> bool {
    ctx.memory(|m| {
        [query_id(), replace_id(), include_id(), exclude_id()]
            .into_iter()
            .any(|id| m.has_focus(id))
    })
}

impl Panel {
    /// Ctrl+Shift+F: shows the panel and focuses its search box, seeded
    /// from the editor's selection.
    pub(crate) fn show_search(&mut self, workspace: &Workspace) {
        if let Some(seed) = workspace.search_seed(self.options.regex) {
            self.query = seed;
        }
        self.shown = true;
        self.focus_query = true;
    }

    /// Ctrl+Shift+H: the panel with its replace box shown.
    pub(crate) fn show_replace(&mut self, workspace: &Workspace) {
        self.show_search(workspace);
        self.replace_shown = true;
    }

    /// `search.action.replaceAll`: the question to confirm, or why there
    /// is nothing to replace.
    pub(crate) fn ask_replace_all(&mut self, workspace: &Workspace) -> Option<String> {
        self.confirm = workspace.replace_confirmation(ReplaceScope::All);
        self.confirm
            .is_none()
            .then(|| "There is nothing to replace".to_owned())
    }

    /// A replace command on the result last clicked; returns a message.
    pub(crate) fn run_selected(&mut self, workspace: &mut Workspace, act: Act) -> Option<String> {
        if act == Act::Undo {
            return Some(match workspace.undo_replace() {
                Ok(files) => format!("Undid the replace in {files} file(s)"),
                Err(err) => err.to_string(),
            });
        }
        let Some(row) = self.selected else {
            return Some("Select a match or a file first".to_owned());
        };
        self.act(workspace, row, act)
    }

    /// Does `act` on `row`; returns a message when it failed.
    fn act(&mut self, workspace: &mut Workspace, row: SearchRow, act: Act) -> Option<String> {
        workspace.set_search_replacement(self.replace_text());
        let (file, index) = match row {
            SearchRow::File { file, .. } => (file, None),
            SearchRow::Match { file, index } => (file, Some(index)),
        };
        match (act, index) {
            (Act::Remove, Some(index)) => {
                workspace.dismiss_search_match(file, index);
            }
            (Act::Remove, None) => {
                workspace.dismiss_search_file(file);
            }
            (Act::Replace, Some(index)) => {
                return workspace
                    .start_replace(ReplaceScope::Match { file, index })
                    .err()
                    .map(|e| e.to_string());
            }
            (Act::Replace | Act::ReplaceFile, _) => {
                return workspace
                    .start_replace(ReplaceScope::File(file))
                    .err()
                    .map(|e| e.to_string());
            }
            (Act::Undo, _) => {}
        }
        None
    }

    /// The replace box's text, or nothing while the box is hidden.
    fn replace_text(&self) -> &str {
        if self.replace_shown {
            &self.replacement
        } else {
            ""
        }
    }

    /// A replace ended: the message for the status bar.
    pub(crate) fn replaced(&mut self, workspace: &mut Workspace) -> Option<String> {
        let replacement = workspace.search_replacement().to_owned();
        self.selected = None;
        Some(match workspace.take_replace_outcome()? {
            Ok(done) if done.unsaved.is_empty() => done.message(&replacement),
            Ok(done) => format!(
                "{} Not saved: {}",
                done.message(&replacement),
                done.unsaved.join("; ")
            ),
            Err(err) => err.to_string(),
        })
    }

    /// Focuses the search box on the next frame.
    pub(crate) fn focus(&mut self) {
        self.focus_query = true;
    }

    /// Starts a search for the boxes' text; a running one stops first.
    fn run(&mut self, workspace: &mut Workspace) {
        let find = match FindQuery::new(&self.query, self.options) {
            Ok(find) => find,
            Err(err) => {
                self.error = Some(err.to_string());
                return;
            }
        };
        let mut query = SearchQuery::new(find);
        query.include.clone_from(&self.include);
        query.exclude.clone_from(&self.exclude);
        self.error = workspace.start_search(query).err().map(|e| e.to_string());
    }
}

fn text(text: impl Into<String>, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::proportional(FONT_SIZE))
        .color(color)
}

fn label(ui: &mut Ui, rich: RichText) {
    ui.add(Label::new(rich).selectable(false).truncate());
}

/// Alt+C, Alt+W, Alt+R while a box has focus: toggles the option and
/// takes the key (and the letter it may type) out of the frame's input.
fn toggle_keys(ui: &Ui, panel: &mut Panel) -> bool {
    if !has_focus(ui.ctx()) {
        return false;
    }
    let mut toggled = false;
    for (key, letter) in [(Key::C, "c"), (Key::W, "w"), (Key::R, "r")] {
        let pressed = ui.ctx().input_mut(|input| {
            let pressed = input.consume_key(Modifiers::ALT, key);
            if pressed {
                input
                    .events
                    .retain(|event| !matches!(event, egui::Event::Text(t) if t.eq_ignore_ascii_case(letter)));
            }
            pressed
        });
        if pressed {
            let options = &mut panel.options;
            match key {
                Key::C => options.match_case = !options.match_case,
                Key::W => options.whole_word = !options.whole_word,
                _ => options.regex = !options.regex,
            }
            toggled = true;
        }
    }
    toggled
}

/// Draws the panel over the whole of `ui`.
pub(crate) fn show(ui: &mut Ui, workspace: &mut Workspace, panel: &mut Panel) -> Outcome {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, BACKGROUND);
    ui.painter()
        .vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0, BORDER));
    let mut outcome = Outcome::default();
    if has_focus(ui.ctx())
        && ui
            .ctx()
            .input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
        && !workspace.cancel_search()
    {
        outcome.focus_editor = true;
    }
    let mut rerun = toggle_keys(ui, panel);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(12.0, 8.0))),
        |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            label(ui, text("SEARCH", FOREGROUND).strong());
            let (enter, clicked) = search_box(ui, panel);
            rerun |= clicked;
            if panel.replace_shown {
                if replace_row(ui, workspace, panel) {
                    outcome.message = panel.ask_replace_all(workspace);
                }
                confirm_row(ui, workspace, panel, &mut outcome);
            }
            let enter = enter | field(ui, &mut panel.include, include_id(), "files to include");
            let enter = enter | field(ui, &mut panel.exclude, exclude_id(), "files to exclude");
            if enter || (rerun && workspace.search_results().query().is_some()) {
                panel.run(workspace);
            }
            status(ui, workspace, panel);
            let shown = results(ui, workspace, panel, &mut outcome.focus_editor);
            outcome.message = shown.or(outcome.message.take());
        },
    );
    outcome
}

/// The search box and its toggles; returns whether Enter was pressed in
/// it and whether a toggle was clicked.
fn search_box(ui: &mut Ui, panel: &mut Panel) -> (bool, bool) {
    ui.horizontal(|ui| {
        let mut clicked = false;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            for (on, caption, title) in [
                (
                    &mut panel.options.regex,
                    ".*",
                    "Use Regular Expression (Alt+R)",
                ),
                (
                    &mut panel.options.whole_word,
                    "ab",
                    "Match Whole Word (Alt+W)",
                ),
                (&mut panel.options.match_case, "Aa", "Match Case (Alt+C)"),
            ] {
                let button =
                    Button::selectable(*on, text(caption, FOREGROUND)).stroke(Stroke::NONE);
                if ui.add(button).on_hover_text(title).clicked() {
                    *on = !*on;
                    clicked = true;
                }
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let enter = field(ui, &mut panel.query, query_id(), "Search");
                if panel.focus_query {
                    panel.focus_query = false;
                    ui.memory_mut(|m| m.request_focus(query_id()));
                }
                // A click on a toggle gives focus back to the search box on
                // the next frame, as VS Code's find input does
                // (`src/vs/base/browser/ui/findinput/findInput.ts:139-141`).
                if clicked {
                    panel.focus_query = true;
                    ui.ctx().request_repaint();
                }
                (enter, clicked)
            })
            .inner
        })
        .inner
    })
    .inner
}

/// The replace box with its Replace All button; returns whether Replace
/// All was asked, by the button or by Enter in the box.
fn replace_row(ui: &mut Ui, workspace: &mut Workspace, panel: &mut Panel) -> bool {
    let ask = ui
        .horizontal(|ui| {
            let mut ask = false;
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let idle =
                    workspace.search_results().match_count() > 0 && !workspace.is_replacing();
                let button = Button::new(text("Replace All", FOREGROUND)).stroke(Stroke::NONE);
                ask |= ui.add_enabled(idle, button).clicked();
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ask |= field(ui, &mut panel.replacement, replace_id(), "Replace");
                });
            });
            ask
        })
        .inner;
    workspace.set_search_replacement(panel.replace_text());
    ask
}

/// The question of a Replace All that waits, with its two buttons.
fn confirm_row(ui: &mut Ui, workspace: &mut Workspace, panel: &mut Panel, outcome: &mut Outcome) {
    let Some(question) = panel.confirm.clone() else {
        return;
    };
    ui.add(
        Label::new(text(question, FOREGROUND))
            .selectable(false)
            .wrap(),
    );
    ui.horizontal(|ui| {
        if ui.button(text("Confirm", FOREGROUND)).clicked() {
            panel.confirm = None;
            outcome.message = workspace
                .start_replace(ReplaceScope::All)
                .err()
                .map(|err| err.to_string());
        }
        if ui.button(text("Cancel", FOREGROUND)).clicked() {
            panel.confirm = None;
        }
    });
}

/// One text box; returns whether Enter was pressed in it.
fn field(ui: &mut Ui, value: &mut String, id: Id, hint: &str) -> bool {
    let response = ui.add(
        TextEdit::singleline(value)
            .id(id)
            .hint_text(text(hint, DESCRIPTION))
            .font(FontId::proportional(FONT_SIZE))
            .text_color(FOREGROUND)
            .background_color(INPUT)
            .return_key(None)
            .desired_width(f32::INFINITY),
    );
    response.has_focus() && ui.input(|input| input.key_pressed(Key::Enter))
}

/// The line under the boxes: why the search could not start, the count or
/// why there are no results, and the limit warning.
fn status(ui: &mut Ui, workspace: &Workspace, panel: &Panel) {
    let results = workspace.search_results();
    let replacing = workspace
        .replace_progress()
        .map(|p| format!("Replacing… {}/{}", p.done, p.total));
    let line = panel
        .error
        .clone()
        .or(replacing)
        .or_else(|| results.message())
        .or_else(|| results.is_running().then(|| "Searching…".to_owned()));
    if let Some(line) = line {
        label(ui, text(line, DESCRIPTION));
    }
    if let Some(warning) = results.warning() {
        ui.add(
            Label::new(text(warning, DESCRIPTION))
                .selectable(false)
                .wrap(),
        );
    }
}

/// What a click on a result row asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Click {
    /// The row itself: a file shows or hides its matches, a match opens.
    Row,
    /// Its Replace button.
    Replace,
    /// Its Dismiss button.
    Dismiss,
}

/// The results list; returns a message when a match could not be opened.
fn results(
    ui: &mut Ui,
    workspace: &mut Workspace,
    panel: &mut Panel,
    focus_editor: &mut bool,
) -> Option<String> {
    let rows = workspace.search_results().rows();
    let mut clicked = None;
    let replace = panel.replace_shown;
    ScrollArea::vertical()
        .auto_shrink(false)
        .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
            for index in range {
                if let Some(click) = row(ui, workspace, rows[index], replace) {
                    clicked = Some((rows[index], click));
                }
            }
        });
    let (found, click) = clicked?;
    panel.selected = Some(found);
    match (click, found) {
        (Click::Replace, _) => panel.act(workspace, found, Act::Replace),
        (Click::Dismiss, _) => panel.act(workspace, found, Act::Remove),
        (Click::Row, SearchRow::File { file, .. }) => {
            workspace.toggle_search_file(file);
            None
        }
        (Click::Row, SearchRow::Match { file, index }) => {
            match workspace.open_search_match(file, index) {
                Ok(_) => {
                    *focus_editor = true;
                    None
                }
                Err(err) => Some(err.to_string()),
            }
        }
    }
}

/// The Replace and Dismiss buttons of a row; returns which was clicked.
fn row_buttons(ui: &mut Ui, busy: bool) -> Option<Click> {
    let mut click = None;
    ui.add_enabled_ui(!busy, |ui| {
        if ui.small_button(text("Dismiss", DESCRIPTION)).clicked() {
            click = Some(Click::Dismiss);
        }
        if ui.small_button(text("Replace", DESCRIPTION)).clicked() {
            click = Some(Click::Replace);
        }
    });
    click
}

/// One row, `ROW_HEIGHT` high; returns what was clicked in it.
fn row(ui: &mut Ui, workspace: &Workspace, row: SearchRow, replace: bool) -> Option<Click> {
    let size = Vec2::new(ui.available_width(), ROW_HEIGHT);
    let busy = workspace.is_replacing();
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(size);
        let results = workspace.search_results();
        match row {
            SearchRow::File { file, expanded } => {
                let found = results.files().get(file)?;
                let (twistie, _) =
                    ui.allocate_exact_size(Vec2::new(16.0, ROW_HEIGHT), Sense::hover());
                crate::explorer::chevron(ui.painter(), twistie.center(), expanded);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut click = None;
                    if replace {
                        click = row_buttons(ui, busy);
                    }
                    label(ui, text(found.matches.len().to_string(), DESCRIPTION));
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let name = text(found.label.clone(), FOREGROUND);
                        if ui.add(Button::new(name).frame(false)).clicked() {
                            click = Some(Click::Row);
                        }
                        label(ui, text(found.description.clone(), DESCRIPTION));
                    });
                    click
                })
                .inner
            }
            SearchRow::Match { file, index } => {
                let found = results
                    .files()
                    .get(file)
                    .and_then(|f| f.matches.get(index))?;
                ui.add_space(MATCH_INDENT);
                let mut click = None;
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if replace {
                        click = row_buttons(ui, busy);
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        // Keep a long line out from under the buttons.
                        let room = ui.available_rect_before_wrap().intersect(ui.clip_rect());
                        ui.set_clip_rect(room);
                        let replacement = replace
                            .then(|| workspace.search_replacement_text(file, index))
                            .flatten();
                        let job = match_job(found, replacement.as_deref(), ui.available_width());
                        if ui.add(Button::new(job).frame(false)).clicked() {
                            click = Some(Click::Row);
                        }
                    });
                });
                click
            }
        }
    })
    .inner
}

/// A match's line: the matched text highlighted, or when a replace text is
/// known, struck through with the text that replaces it after it.
fn match_job(
    found: &rustode_workspace::SearchMatch,
    replacement: Option<&str>,
    width: f32,
) -> LayoutJob {
    let preview = found.preview();
    let mut job = LayoutJob::default();
    let plain = TextFormat::simple(FontId::proportional(FONT_SIZE), FOREGROUND);
    job.append(&preview.before.replace('\t', " "), 0.0, plain.clone());
    match replacement {
        None => {
            let highlight = TextFormat {
                background: MATCH,
                ..plain.clone()
            };
            job.append(&preview.inside, 0.0, highlight);
        }
        Some(added) => {
            let removed = TextFormat {
                background: REMOVED,
                strikethrough: Stroke::new(1.0, FOREGROUND),
                ..plain.clone()
            };
            let inserted = TextFormat {
                background: ADDED,
                ..plain.clone()
            };
            job.append(&preview.inside, 0.0, removed);
            job.append(&added.replace(['\t', '\n'], " "), 0.0, inserted);
        }
    }
    job.append(&preview.after.replace('\t', " "), 0.0, plain);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.max_width = width;
    job
}
