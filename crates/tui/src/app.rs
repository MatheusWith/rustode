//! The TUI's state and event handling.

use crate::keys::{self, Command, ExplorerKey};
use crate::theme::{ColorMode, Theme};
use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::{DefaultTerminal, Frame, Terminal};
use rustode_workspace::trace::{LatencyRecorder, LatencyReport};
use rustode_workspace::{
    CommandMatch, ContextValue, Document, DocumentId, Error, Event as WorkspaceEvent, ExplorerRow,
    FileMatch, Movement, Position, QuickQuery, Resolution, Selection, ServerState, Workspace,
    parse_quick_query,
};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often the idle event loop checks the disk for external changes.
const DISK_POLL: Duration = Duration::from_millis(250);
/// How often it takes search results while a search runs.
const SEARCH_POLL: Duration = Duration::from_millis(30);

/// Which part of the screen receives keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    Editor,
    Explorer,
    /// The Extensions view in the sidebar.
    Extensions,
    Problems,
    /// The Search view in the sidebar.
    Search,
}

/// Where a document's view is scrolled to.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Viewport {
    /// First visible line.
    pub top: usize,
    /// First visible display column.
    pub left: usize,
    /// Where the cursor was when the view last followed it; the view only
    /// follows the cursor again once it moves, so wheel scrolling sticks.
    pub revealed: Option<Position>,
}

/// The editor's text area on the last frame, for mouse clicks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EditorHit {
    pub area: Rect,
    pub gutter: u16,
    pub id: DocumentId,
    pub tab_size: usize,
}

/// What was drawn where on the last frame.
#[derive(Debug, Clone, Default)]
pub(crate) struct Hits {
    pub editor: Option<EditorHit>,
    /// Each tab's row, first and one-past-last column.
    pub tabs: Vec<(u16, u16, u16, DocumentId)>,
    /// The explorer's rows.
    pub explorer: Option<Rect>,
    /// The problems panel's rows, and the problem each one shows.
    pub problems: Option<(Rect, Vec<Option<usize>>)>,
    /// The status bar's problem counts: row, first and one-past-last column.
    pub problems_status: Option<(u16, u16, u16)>,
    /// The status bar's language name: row, first and one-past-last column.
    pub language_status: Option<(u16, u16, u16)>,
    /// The Search view's result rows.
    pub search: Option<Rect>,
}

/// Lines one wheel notch scrolls.
const WHEEL_LINES: usize = 3;

/// The quick input box (Ctrl+P, F1, Ctrl+G): its text and selected item.
#[derive(Debug, Clone, Default)]
pub(crate) struct QuickInput {
    pub text: String,
    pub selected: usize,
    /// Picking the language of this document ("Change Language Mode")
    /// instead of a file, command or line.
    pub language: Option<DocumentId>,
}

/// One row of the quick input list.
#[derive(Debug, Clone)]
pub(crate) enum QuickItem {
    File(FileMatch),
    Command(CommandMatch),
    Line {
        line: usize,
        column: Option<usize>,
    },
    Hint(String),
    /// A language of the "Change Language Mode" picker.
    Language(rustode_workspace::LanguagePick),
}

/// Rows the quick input list shows at most.
pub(crate) const QUICK_ROWS: usize = 10;

/// The terminal editor: open documents in tabs, an optional folder in the
/// explorer, and the time from each key to the frame that shows it.
#[derive(Debug)]
pub struct App {
    pub(crate) workspace: Workspace,
    pub(crate) focus: Focus,
    /// Scroll position per document, kept when switching tabs.
    pub(crate) views: HashMap<DocumentId, Viewport>,
    /// Lines Page Up/Down move: the editor's height minus 2, as in VS Code
    /// (`src/vs/editor/common/cursorCommon.ts:129`).
    pub(crate) page: usize,
    /// The explorer's selected row and first visible row.
    pub(crate) explorer_row: usize,
    pub(crate) explorer_top: usize,
    /// The selected row when the explorer last scrolled to it.
    pub(crate) explorer_revealed: Option<usize>,
    /// What was drawn where on the last frame.
    pub(crate) hits: Hits,
    /// Where a mouse drag started, as a char offset.
    drag_anchor: Option<usize>,
    /// Explorer rows that fit on screen, from the last frame.
    pub(crate) explorer_height: usize,
    /// One line for the status bar: a save result, an error or a prompt.
    pub(crate) message: Option<String>,
    /// The open quick input box, if any.
    pub(crate) quick: Option<QuickInput>,
    /// Colors for the terminal's color mode.
    pub(crate) theme: Theme,
    /// The find widget's state, kept when closed so F3 repeats the search.
    pub(crate) find: Option<crate::find::FindWidget>,
    /// Whether the find widget is shown.
    pub(crate) find_visible: bool,
    /// The Extensions view's state.
    pub(crate) extensions: crate::extensions::ExtensionsView,
    /// The problems panel.
    pub(crate) problems: crate::problems::ProblemsPanel,
    /// The Search view's state.
    pub(crate) search: crate::search::SearchView,
    /// The hover and suggest popups' keys.
    pub(crate) suggest: crate::suggest::State,
    confirm_quit: bool,
    confirm_close: Option<DocumentId>,
    quit: bool,
    latency: LatencyRecorder,
}

impl App {
    /// Opens a file, or a folder in the explorer.
    ///
    /// # Errors
    /// When the path cannot be read.
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::open_paths(&[path.to_owned()])
    }

    /// Opens files in tabs and the first folder in the explorer.
    ///
    /// # Errors
    /// When a path cannot be read.
    pub fn open_paths(paths: &[PathBuf]) -> Result<Self, Error> {
        let mut workspace = Workspace::new();
        for path in paths {
            if path.is_dir() {
                if workspace.explorer().is_none() {
                    workspace.open_folder(path)?;
                }
            } else {
                workspace.open(path)?;
            }
        }
        let focus = if workspace.active_document().is_none() && workspace.explorer().is_some() {
            Focus::Explorer
        } else {
            Focus::Editor
        };
        Ok(Self {
            workspace,
            focus,
            views: HashMap::new(),
            page: 1,
            explorer_row: 0,
            explorer_top: 0,
            explorer_revealed: None,
            hits: Hits::default(),
            drag_anchor: None,
            explorer_height: 1,
            message: None,
            quick: None,
            find: None,
            find_visible: false,
            extensions: crate::extensions::ExtensionsView::default(),
            problems: crate::problems::ProblemsPanel::default(),
            search: crate::search::SearchView::default(),
            suggest: crate::suggest::State::default(),
            theme: Theme::new(ColorMode::detect(|key| std::env::var(key).ok())),
            confirm_quit: false,
            confirm_close: None,
            quit: false,
            latency: LatencyRecorder::new(),
        })
    }

    /// Draws with `mode`'s colors instead of the detected ones.
    pub fn set_color_mode(&mut self, mode: ColorMode) {
        self.theme = Theme::new(mode);
    }

    /// The workspace holding the documents.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// The workspace, to configure it (e.g. its extensions folder).
    pub fn workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspace
    }

    /// Takes the results of background work (extension searches,
    /// installs, uninstalls, search results). Returns whether anything
    /// changed, so the screen needs drawing.
    pub fn poll(&mut self) -> bool {
        self.workspace.poll_extensions();
        self.workspace.poll_search();
        self.take_events()
    }

    /// The document in the active tab.
    #[must_use]
    pub fn document(&self) -> Option<DocumentId> {
        self.workspace.active_document()
    }

    /// Whether the user asked to quit.
    #[must_use]
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// Key-to-frame latency so far: from receiving a key or paste to the end
    /// of writing the frame that shows it.
    #[must_use]
    pub fn latency(&self) -> Option<LatencyReport> {
        self.latency.report()
    }

    /// Draws the screen from workspace state.
    pub fn draw(&mut self, frame: &mut Frame) {
        crate::view::draw(self, frame);
    }

    /// Handles one event, draws the frame that shows it, and records the
    /// key-to-frame time for keys and pastes.
    ///
    /// # Errors
    /// When the terminal cannot be drawn.
    pub fn step<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        event: &Event,
    ) -> Result<(), B::Error> {
        let received = Instant::now();
        self.handle(event);
        terminal.draw(|frame| self.draw(frame))?;
        if matches!(event, Event::Key(_) | Event::Paste(_)) {
            self.latency.record_between(received, Instant::now());
        }
        Ok(())
    }

    /// Runs the event loop until the user quits. While idle it wakes every
    /// 250 ms to pick up changes made to files by other programs and what the
    /// language servers sent.
    ///
    /// # Errors
    /// When the terminal cannot be read or drawn.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        terminal.draw(|frame| self.draw(frame))?;
        while !self.quit {
            let wait = if self.workspace.search_results().is_running() {
                SEARCH_POLL
            } else {
                DISK_POLL
            };
            if event::poll(wait)? {
                let event = event::read()?;
                self.step(terminal, &event)?;
            } else {
                self.workspace.poll_external_changes();
                self.workspace.poll_extensions();
                let servers = self.poll_language_servers();
                self.workspace.poll_search();
                let config_changed = !self.workspace.reload_config().is_empty();
                if config_changed {
                    self.report_config_problems();
                }
                if self.take_events() || config_changed || servers {
                    terminal.draw(|frame| self.draw(frame))?;
                }
            }
        }
        Ok(())
    }

    /// Processes what the language servers sent; returns whether anything
    /// changed, so the screen needs drawing.
    pub fn poll_language_servers(&mut self) -> bool {
        self.workspace.poll_language_servers();
        self.take_events()
    }

    /// Applies one terminal event to the workspace.
    pub fn handle(&mut self, event: &Event) {
        self.workspace.poll_language_servers();
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release && self.quick.is_some() => {
                self.quick_key(*key);
            }
            Event::Paste(text) if self.quick.is_some() => {
                if let Some(quick) = self.quick.as_mut() {
                    quick.text.push_str(text.lines().next().unwrap_or_default());
                    quick.selected = 0;
                }
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => self.key(*key),
            Event::Mouse(mouse) => self.mouse(*mouse),
            Event::Paste(text) if self.focus == Focus::Extensions => self.extensions_paste(text),
            Event::Paste(text) if self.focus == Focus::Search => self.search_paste(text),
            Event::Paste(text) if self.focus == Focus::Editor => {
                self.confirm_quit = false;
                self.message = None;
                self.report(|ws, id| ws.type_text(id, text));
            }
            _ => {}
        }
        self.workspace.poll_extensions();
        self.take_events();
    }

    /// A key: the user's `keybindings.json` first (later entries win, a
    /// false `when` falls through), then the default keys unless the user
    /// removed them (`"-command"`).
    fn key(&mut self, key: KeyEvent) {
        if self.find_visible && self.focus == Focus::Editor && self.find_key(key) {
            return;
        }
        if let Some(id) = self.user_binding(key) {
            self.message = None;
            self.run_id(&id);
            return;
        }
        if self.focus == Focus::Editor && self.suggest_key(key) {
            return;
        }
        match self.focus {
            Focus::Editor => {
                if let Some(command) = keys::command(key).filter(|c| !self.removed(c, key)) {
                    self.run_command(command);
                }
            }
            Focus::Explorer | Focus::Problems => {
                if let Some(command) = keys::global(key) {
                    self.run_global(command, key);
                } else if let Some(list_key) = keys::explorer_key(key) {
                    self.message = None;
                    if self.focus == Focus::Problems {
                        self.problems_key(list_key);
                    } else {
                        self.explorer_key(list_key);
                    }
                }
            }
            Focus::Extensions => self.extensions_key(key),
            Focus::Search => self.search_key(key),
        }
    }

    /// Runs a key's workbench command unless the user removed its key.
    pub(crate) fn run_global(&mut self, command: Command, key: KeyEvent) {
        if !self.removed(&command, key) {
            self.run_command(command);
        }
    }

    /// The command of the last user keybinding for `key` whose `when` holds.
    fn user_binding(&self, key: KeyEvent) -> Option<String> {
        let context = self.context();
        self.workspace
            .config()
            .keybindings()
            .iter()
            .rev()
            .filter(|b| !b.is_removal())
            .filter(
                |b| matches!(b.key.as_deref(), Some([chord]) if keys::matches_chord(key, chord)),
            )
            .find(|b| b.when.as_ref().is_none_or(|w| w.evaluate(&context)))
            .and_then(|b| b.command.clone())
    }

    /// Whether the user removed this default key for `command`.
    fn removed(&self, command: &Command, key: KeyEvent) -> bool {
        let Some(id) = keys::id_of(command) else {
            return false;
        };
        self.workspace.config().keybindings().iter().any(|b| {
            b.command.as_deref().and_then(|c| c.strip_prefix('-')) == Some(id)
                && match b.key.as_deref() {
                    None => true,
                    Some([chord]) => keys::matches_chord(key, chord),
                    Some(_) => false,
                }
        })
    }

    /// Context keys for `when` clauses, named as in VS Code.
    fn context(&self) -> impl Fn(&str) -> ContextValue + '_ {
        let document = self.document().and_then(|id| self.workspace.document(id));
        let editor = self.focus == Focus::Editor && self.quick.is_none();
        let explorer = self.focus == Focus::Explorer && self.quick.is_none();
        move |key: &str| {
            let flag = ContextValue::Bool;
            match key {
                "editorTextFocus" | "editorFocus" | "textInputFocus" => flag(editor),
                "filesExplorerFocus" | "explorerViewletFocus" | "listFocus" | "sideBarFocus" => {
                    flag(explorer)
                }
                "explorerViewletVisible" => flag(self.workspace.explorer().is_some()),
                "inQuickOpen" => flag(self.quick.is_some()),
                "editorHasSelection" => {
                    flag(document.is_some_and(|d| !d.primary_selection().is_empty()))
                }
                "editorHasMultipleSelections" => {
                    flag(document.is_some_and(|d| d.selections().len() > 1))
                }
                "editorLangId" => document.map_or(ContextValue::Undefined, |d| {
                    ContextValue::Str(d.language_id().to_owned())
                }),
                "isLinux" => flag(true),
                "isMac" | "isWindows" | "editorReadonly" => flag(false),
                _ => ContextValue::Undefined,
            }
        }
    }

    /// Runs a command by VS Code id: through the TUI's own handling when a
    /// key could run it, else through the workspace's registry.
    fn run_id(&mut self, id: &str) {
        if let Some(command) = keys::from_id(id) {
            self.run_command(command);
            return;
        }
        match self.workspace.execute(id) {
            Ok(true) => {}
            Ok(false) => {
                self.message = Some(format!("{id} is not available in the terminal editor"));
            }
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    /// Reads `settings.json` and `keybindings.json` from `dir` (VS Code's
    /// `User` folder layout); problems show in the status bar.
    pub fn load_user_config(&mut self, dir: &Path) {
        let _ = self
            .workspace
            .load_config(&dir.join("settings.json"), &dir.join("keybindings.json"));
        self.report_config_problems();
    }

    fn report_config_problems(&mut self) {
        let problems = self.workspace.config().diagnostics().count()
            + self.workspace.language_server_problems().len();
        if problems > 0 {
            self.message = Some(format!(
                "{problems} problem(s) in settings.json or keybindings.json"
            ));
        }
    }

    fn run_command(&mut self, command: Command) {
        if command != Command::Quit {
            self.confirm_quit = false;
        }
        if command != Command::CloseTab {
            self.confirm_close = None;
        }
        self.message = None;
        match command {
            Command::Save => self.save(),
            Command::Quit => self.quit_or_confirm(),
            Command::CloseTab => self.close_or_confirm(),
            Command::NextTab => self.workspace.next_tab(),
            Command::PreviousTab => self.workspace.previous_tab(),
            Command::FocusExplorer => self.focus_explorer(),
            Command::FocusNextPart => self.focus_next_part(),
            Command::ShowExtensions => self.show_extensions(),
            Command::ShowSearch => self.show_search(),
            Command::Reload => self.reload(),
            Command::QuickOpen => self.open_quick(""),
            Command::ShowCommands => self.open_quick(">"),
            Command::GotoLine => self.open_quick(":"),
            Command::Find => self.open_find(false),
            Command::Replace => self.open_find(true),
            Command::FindNext => self.find_step(true),
            Command::FindPrevious => self.find_step(false),
            Command::ToggleProblems => self.toggle_problems(),
            Command::ChangeLanguage => self.open_language_picker(),
            other => self.edit(other),
        }
    }

    /// Editing commands, which need an active document.
    fn edit(&mut self, command: Command) {
        match command {
            Command::Type(text) => self.report(|ws, id| ws.type_text(id, &text)),
            Command::Enter => self.report(Workspace::enter),
            Command::Tab => self.report(Workspace::tab),
            Command::DeleteLeft => self.report(Workspace::delete_left),
            Command::DeleteRight => self.report(Workspace::delete_right),
            Command::DeleteWordLeft => self.report(Workspace::delete_word_left),
            Command::DeleteWordRight => self.report(Workspace::delete_word_right),
            Command::Move(movement, select) => {
                self.report(|ws, id| ws.move_cursor(id, movement, select));
            }
            Command::Page { up, select } => {
                let movement = if up {
                    Movement::PageUp(self.page)
                } else {
                    Movement::PageDown(self.page)
                };
                self.report(|ws, id| ws.move_cursor(id, movement, select));
            }
            Command::Undo => self.report(|ws, id| ws.undo(id).map(drop)),
            Command::Redo => self.report(|ws, id| ws.redo(id).map(drop)),
            Command::SelectAll => self.report(Workspace::select_all),
            Command::AddNextOccurrence => {
                self.report(|ws, id| ws.add_next_occurrence(id).map(drop));
            }
            Command::AddCursorAbove => self.report(|ws, id| ws.add_cursor_above(id).map(drop)),
            Command::AddCursorBelow => self.report(|ws, id| ws.add_cursor_below(id).map(drop)),
            Command::Escape => self.report(|ws, id| ws.cancel_selections(id).map(drop)),
            _ => {}
        }
    }

    /// Runs a command on the active document and shows its error, if any.
    fn report(&mut self, f: impl FnOnce(&mut Workspace, DocumentId) -> Result<(), Error>) {
        let Some(id) = self.document() else { return };
        if let Err(err) = f(&mut self.workspace, id) {
            self.message = Some(err.to_string());
        }
    }

    /// Ctrl+S. After the file changed on disk under unsaved edits, saving
    /// keeps the user's version (the conflict prompt says so).
    fn save(&mut self) {
        let Some(id) = self.document() else { return };
        let result = match self.workspace.save(id) {
            Err(Error::Conflict(_)) => self
                .workspace
                .resolve_conflict(id, Resolution::KeepMine)
                .and_then(|()| self.workspace.save(id)),
            other => other,
        };
        self.message = Some(match result {
            Ok(()) => "Saved".to_owned(),
            Err(err) => err.to_string(),
        });
    }

    /// F5: loads the disk version after a conflict.
    fn reload(&mut self) {
        let Some(id) = self.document() else { return };
        if self
            .workspace
            .document(id)
            .is_some_and(Document::has_conflict)
        {
            self.report(|ws, id| ws.resolve_conflict(id, Resolution::ReloadTheirs));
        }
    }

    fn quit_or_confirm(&mut self) {
        let dirty = self
            .workspace
            .tabs()
            .iter()
            .any(|&id| self.workspace.document(id).is_some_and(Document::is_dirty));
        if !dirty || self.confirm_quit {
            self.quit = true;
        } else {
            self.confirm_quit = true;
            self.message =
                Some("Unsaved changes: Ctrl+Q again quits without saving, Ctrl+S saves".to_owned());
        }
    }

    /// Ctrl+W. A dirty document asks first: a second Ctrl+W discards.
    fn close_or_confirm(&mut self) {
        let Some(id) = self.document() else { return };
        let discard = self.confirm_close == Some(id);
        match self.workspace.close(id, discard) {
            Ok(()) => {
                self.confirm_close = None;
                self.views.remove(&id);
                if self.document().is_none() && self.workspace.explorer().is_some() {
                    self.focus = Focus::Explorer;
                }
            }
            Err(Error::Dirty(_)) => {
                self.confirm_close = Some(id);
                self.message = Some(
                    "Unsaved changes: Ctrl+W again closes without saving, Ctrl+S saves".to_owned(),
                );
            }
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    fn focus_explorer(&mut self) {
        if self.workspace.explorer().is_none() {
            self.message = Some("No folder opened".to_owned());
            return;
        }
        self.focus = Focus::Explorer;
        self.extensions.shown = false;
        self.search.shown = false;
        let active = self
            .document()
            .and_then(|id| self.workspace.document(id))
            .map(|d| d.path().to_owned());
        if let Some(index) = active.and_then(|path| self.rows().iter().position(|r| r.path == path))
        {
            self.explorer_row = index;
        }
    }

    fn focus_next_part(&mut self) {
        match self.focus {
            Focus::Editor => self.focus_explorer(),
            Focus::Explorer | Focus::Extensions | Focus::Problems | Focus::Search
                if self.document().is_some() =>
            {
                self.focus = Focus::Editor;
            }
            Focus::Explorer | Focus::Extensions | Focus::Problems | Focus::Search => {}
        }
    }

    /// Ctrl+Shift+M: shows and focuses the problems panel, or hides it when
    /// it has focus.
    fn toggle_problems(&mut self) {
        if self.focus == Focus::Problems {
            self.problems.visible = false;
            self.focus = Focus::Editor;
        } else {
            self.problems.visible = true;
            self.focus = Focus::Problems;
        }
    }

    /// Keys while the problems panel has focus.
    fn problems_key(&mut self, key: ExplorerKey) {
        let count = self.workspace.problems().len();
        let last = count.saturating_sub(1);
        let selected = self.problems.selected.min(last);
        self.problems.selected = match key {
            ExplorerKey::Up => selected.saturating_sub(1),
            ExplorerKey::Down => (selected + 1).min(last),
            ExplorerKey::PageUp => selected.saturating_sub(self.page),
            ExplorerKey::PageDown => (selected + self.page).min(last),
            ExplorerKey::Home => 0,
            ExplorerKey::End => last,
            ExplorerKey::Open { .. } => {
                self.go_to_problem(selected);
                return;
            }
            ExplorerKey::Leave => {
                self.focus = Focus::Editor;
                return;
            }
            ExplorerKey::Collapse | ExplorerKey::Expand => selected,
        };
    }

    /// Opens the `index`th problem's file at the problem, focusing the editor.
    fn go_to_problem(&mut self, index: usize) {
        let Some(problem) = self.workspace.problems().into_iter().nth(index) else {
            return;
        };
        self.problems.selected = index;
        match self.workspace.go_to_problem(&problem) {
            Ok(_) => self.focus = Focus::Editor,
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    pub(crate) fn rows(&self) -> Vec<ExplorerRow> {
        self.workspace
            .explorer()
            .map_or_else(Default::default, rustode_workspace::Explorer::rows)
    }

    fn explorer_key(&mut self, key: ExplorerKey) {
        let rows = self.rows();
        let last = rows.len().saturating_sub(1);
        let row = self.explorer_row.min(last);
        let page = self.explorer_height.max(1);
        match key {
            ExplorerKey::Up => self.explorer_row = row.saturating_sub(1),
            ExplorerKey::Down => self.explorer_row = (row + 1).min(last),
            ExplorerKey::PageUp => self.explorer_row = row.saturating_sub(page),
            ExplorerKey::PageDown => self.explorer_row = (row + page).min(last),
            ExplorerKey::Home => self.explorer_row = 0,
            ExplorerKey::End => self.explorer_row = last,
            ExplorerKey::Leave if self.document().is_some() => self.focus = Focus::Editor,
            ExplorerKey::Leave => {}
            ExplorerKey::Open { keep_focus } => {
                if let Some(entry) = rows.get(row) {
                    self.open_row(entry, keep_focus);
                }
            }
            ExplorerKey::Collapse => self.collapse(&rows, row),
            ExplorerKey::Expand => self.expand(&rows, row),
        }
    }

    fn open_row(&mut self, entry: &ExplorerRow, keep_focus: bool) {
        if entry.is_dir {
            self.toggle(&entry.path);
            return;
        }
        match self.workspace.open(&entry.path) {
            Ok(_) if !keep_focus => self.focus = Focus::Editor,
            Ok(_) => {}
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    /// VS Code's Left: collapse an expanded folder, else select the parent.
    fn collapse(&mut self, rows: &[ExplorerRow], row: usize) {
        let Some(entry) = rows.get(row) else { return };
        if entry.is_dir && entry.expanded {
            self.toggle(&entry.path);
        } else if let Some(parent) = entry.path.parent()
            && let Some(index) = rows.iter().position(|r| r.path == parent)
        {
            self.explorer_row = index;
        }
    }

    /// VS Code's Right: expand a collapsed folder, else go to its first child.
    fn expand(&mut self, rows: &[ExplorerRow], row: usize) {
        let Some(entry) = rows.get(row) else { return };
        if !entry.is_dir {
            return;
        }
        if entry.expanded {
            if rows
                .get(row + 1)
                .is_some_and(|next| next.depth > entry.depth)
            {
                self.explorer_row = row + 1;
            }
        } else {
            self.toggle(&entry.path);
        }
    }

    fn toggle(&mut self, path: &Path) {
        if let Err(err) = self.workspace.toggle_folder(path) {
            self.message = Some(err.to_string());
        }
    }

    fn open_quick(&mut self, prefix: &str) {
        self.quick = Some(QuickInput {
            text: prefix.to_owned(),
            selected: 0,
            language: None,
        });
    }

    /// The rows for the quick input's current text.
    pub(crate) fn quick_items(&mut self) -> Vec<QuickItem> {
        let Some(text) = self.quick.as_ref().map(|q| q.text.clone()) else {
            return Vec::new();
        };
        if let Some(items) = self.language_items(&text) {
            return items;
        }
        match parse_quick_query(&text) {
            QuickQuery::Files(query) => self
                .workspace
                .quick_open(query, QUICK_ROWS)
                .into_iter()
                .map(QuickItem::File)
                .collect(),
            QuickQuery::Commands(query) => self
                .workspace
                .commands(query)
                .into_iter()
                .map(QuickItem::Command)
                .collect(),
            QuickQuery::Line { line, column } => vec![QuickItem::Line { line, column }],
            QuickQuery::NoLine => {
                let lines = self
                    .document()
                    .and_then(|id| self.workspace.document(id))
                    .map_or(0, Document::line_count);
                vec![QuickItem::Hint(format!(
                    "Type a line number between 1 and {lines} to go to"
                ))]
            }
        }
    }

    /// Keys while the quick input box is open: typing filters, Up/Down
    /// select, Enter runs the selected row, Escape closes.
    fn quick_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let count = self.quick_items().len();
        let Some(quick) = self.quick.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.quick = None,
            KeyCode::Enter => self.run_quick(),
            KeyCode::Up => quick.selected = quick.selected.saturating_sub(1),
            KeyCode::Down => quick.selected = (quick.selected + 1).min(count.saturating_sub(1)),
            KeyCode::Char('p') if ctrl => {
                quick.selected = (quick.selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Backspace => {
                quick.text.pop();
                quick.selected = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                quick.text.push(c);
                quick.selected = 0;
            }
            _ => {}
        }
    }

    fn run_quick(&mut self) {
        let selected = self.quick.as_ref().map_or(0, |q| q.selected);
        let item = self.quick_items().into_iter().nth(selected);
        self.quick = None;
        match item {
            Some(QuickItem::File(file)) => match self.workspace.open(&file.path) {
                Ok(_) => self.focus = Focus::Editor,
                Err(err) => self.message = Some(err.to_string()),
            },
            Some(QuickItem::Command(found)) => self.run_id(found.command.id),
            Some(QuickItem::Line { line, column }) => {
                self.focus = Focus::Editor;
                self.report(|ws, id| ws.go_to_line(id, line, column));
            }
            Some(QuickItem::Language(pick)) => self.pick_language(&pick),
            Some(QuickItem::Hint(_)) | None => {}
        }
    }

    fn mouse(&mut self, mouse: MouseEvent) {
        let (column, row) = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.quick = None;
                self.click(column, row, mouse.modifiers);
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let (Some(anchor), Some(offset)) =
                    (self.drag_anchor, self.offset_at(column, row))
                {
                    self.report(|ws, id| {
                        ws.set_selections(id, vec![Selection::new(anchor, offset)], 0)
                    });
                }
            }
            MouseEventKind::Up(_) => self.drag_anchor = None,
            MouseEventKind::ScrollDown => self.wheel(column, row, true),
            MouseEventKind::ScrollUp => self.wheel(column, row, false),
            _ => {}
        }
    }

    /// A left click: a tab activates, an explorer row opens or toggles, the
    /// text places the cursor (Shift extends the selection, Alt adds a
    /// cursor, as with VS Code's default `editor.multiCursorModifier`).
    fn click(&mut self, column: u16, row: u16, modifiers: KeyModifiers) {
        let inside = |area: Rect| area.contains(ratatui::layout::Position::new(column, row));
        if let Some(&(_, _, _, id)) = self
            .hits
            .tabs
            .iter()
            .find(|(y, x0, x1, _)| *y == row && (*x0..*x1).contains(&column))
        {
            let _ = self.workspace.activate(id);
            self.focus = Focus::Editor;
            return;
        }
        if self
            .hits
            .language_status
            .is_some_and(|(y, x0, x1)| y == row && (x0..x1).contains(&column))
        {
            self.open_language_picker();
            return;
        }
        if self
            .hits
            .problems_status
            .is_some_and(|(y, x0, x1)| y == row && (x0..x1).contains(&column))
        {
            self.toggle_problems();
            return;
        }
        if let Some((area, rows)) = self.hits.problems.clone().filter(|(a, _)| inside(*a)) {
            if let Some(&Some(index)) = rows.get(usize::from(row - area.y)) {
                self.go_to_problem(index);
            } else {
                self.focus = Focus::Problems;
            }
            return;
        }
        if let Some(area) = self.hits.search.filter(|&a| inside(a)) {
            self.search_click(usize::from(row - area.y));
            return;
        }
        if let Some(area) = self.hits.explorer.filter(|&a| inside(a)) {
            let index = self.explorer_top + usize::from(row - area.y);
            let rows = self.rows();
            if let Some(entry) = rows.get(index) {
                self.focus = Focus::Explorer;
                self.explorer_row = index;
                self.open_row(entry, true);
            }
            return;
        }
        let Some(offset) = self.offset_at(column, row) else {
            return;
        };
        self.focus = Focus::Editor;
        let Some(id) = self.document() else { return };
        let Some(document) = self.workspace.document(id) else {
            return;
        };
        let primary = document.primary_selection();
        let mut selections = document.selections().to_vec();
        let result = if modifiers.contains(KeyModifiers::ALT) {
            selections.push(Selection::cursor(offset));
            let last = selections.len() - 1;
            self.workspace.set_selections(id, selections, last)
        } else if modifiers.contains(KeyModifiers::SHIFT) {
            self.workspace
                .set_selections(id, vec![Selection::new(primary.anchor, offset)], 0)
        } else {
            self.drag_anchor = Some(offset);
            self.workspace.set_cursor(id, offset)
        };
        if let Err(err) = result {
            self.message = Some(err.to_string());
        }
    }

    /// The char offset under a screen cell of the editor's text.
    fn offset_at(&self, column: u16, row: u16) -> Option<usize> {
        let hit = self.hits.editor?;
        let area = hit.area;
        if row < area.y || row >= area.bottom() || column < area.x || column >= area.right() {
            return None;
        }
        let view = self.views.get(&hit.id).copied().unwrap_or_default();
        let document = self.workspace.document(hit.id)?;
        let line = view.top + usize::from(row - area.y);
        if line >= document.line_count() {
            return Some(document.len_chars());
        }
        let display = view.left + usize::from(column.saturating_sub(area.x + hit.gutter));
        let char_column = crate::view::column_at(&document.line(line), display, hit.tab_size);
        Some(document.offset_of(Position::new(line, char_column)))
    }

    /// The wheel scrolls the part under the pointer, without moving the cursor.
    fn wheel(&mut self, column: u16, row: u16, down: bool) {
        let point = ratatui::layout::Position::new(column, row);
        if let Some(area) = self.hits.explorer.filter(|a| a.contains(point)) {
            let rows = self.rows().len().saturating_sub(usize::from(area.height));
            self.explorer_top = step(self.explorer_top, down, rows);
            return;
        }
        if let Some(hit) = self.hits.editor.filter(|h| h.area.contains(point)) {
            let lines = self
                .workspace
                .document(hit.id)
                .map_or(1, Document::line_count);
            let view = self.views.entry(hit.id).or_default();
            view.top = step(view.top, down, lines.saturating_sub(1));
        }
    }

    /// What the crashed server of `language` says about itself.
    fn server_message(&self, language: &str) -> String {
        let why = self
            .workspace
            .language_servers()
            .into_iter()
            .find_map(|s| s.message.filter(|_| s.language == language))
            .unwrap_or_else(|| "crashed".to_owned());
        format!("Language server for {language}: {why}")
    }

    /// Drains workspace events (they would pile up otherwise) and turns the
    /// ones the user must know about into a message. Returns whether any came.
    fn take_events(&mut self) -> bool {
        let events = self.workspace.take_events();
        for event in &events {
            let message = match event {
                WorkspaceEvent::Conflict { .. } => {
                    "The file changed on disk: Ctrl+S keeps your version, F5 loads the disk version"
                        .to_owned()
                }
                WorkspaceEvent::Deleted { .. } => {
                    "The file was deleted on disk; Ctrl+S writes it again".to_owned()
                }
                WorkspaceEvent::Renamed { to, .. } => {
                    format!("The file was renamed to {}", to.display())
                }
                WorkspaceEvent::Reloaded { .. } => "Reloaded from disk".to_owned(),
                WorkspaceEvent::Extension(event) => match crate::extensions::message(event) {
                    Some(message) => message,
                    None => continue,
                },
                WorkspaceEvent::LanguageServerChanged {
                    language,
                    state: ServerState::Crashed,
                } => self.server_message(language),
                _ => continue,
            };
            self.message = Some(message);
        }
        !events.is_empty()
    }
}

/// `value` moved by one wheel notch, within `0..=max`.
fn step(value: usize, down: bool, max: usize) -> usize {
    if down {
        (value + WHEEL_LINES).min(max)
    } else {
        value.saturating_sub(WHEEL_LINES)
    }
}
