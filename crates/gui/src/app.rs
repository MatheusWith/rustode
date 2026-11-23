//! The window: the sidebar on the left (the explorer, the Extensions view
//! after Ctrl+Shift+X, see `extensions.rs`, or the Search view after
//! Ctrl+Shift+F, see `search.rs`), the tab strip and the
//! editor on the right, the status bar along the bottom, and the title.
//! Everything is drawn from the workspace's state; the window keeps only
//! what is on screen (scroll positions, the selected explorer row, focus).
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`:
//!
//! - the title follows VS Code's default,
//!   `${dirty}${activeEditorShort}${separator}${rootName}${separator}${appName}`,
//!   with `● ` for unsaved changes and ` - ` between the parts, an empty part
//!   dropped with its separator
//!   (`src/vs/workbench/browser/parts/titlebar/windowTitle.ts:41-53`, `:59`;
//!   `src/vs/base/common/labels.ts:347-410`);
//! - hiding the sidebar hands its focus to the editor
//!   (`src/vs/workbench/browser/layout.ts:1993-1998`); how wide it is, and
//!   when it collapses, is `layout.rs`; how focus moves, `focus.rs`.
//!
//! As in the TUI, closing a tab with unsaved changes asks first: a second
//! Ctrl+W, or a second click on its close button, discards them; quitting
//! with unsaved changes asks the same way, with a second Ctrl+Q.
//!
//! Language servers: the window gives the workspace one waker, which
//! requests a repaint, so diagnostics (and extension results) show while
//! the window is idle without a timer, and it polls the servers each frame.
//! Ctrl+Shift+M or a click on the status bar's counts toggles the problems
//! panel below the editor ([`crate::problems`]); going to a problem opens or
//! activates its file's tab.
//!
//! The quick input (`quick_input.rs`) and the find widget (`find.rs`) float
//! above the workbench; `app/overlays.rs` opens them and runs what they ask.

use crate::focus::Part;
use crate::groups;
use crate::keys::{self, Workbench};
use crate::latency::PresentLatency;
use crate::quick_input::QuickInput;
use crate::{
    debug, editor, explorer, extensions, find, focus, language, layout, navigate, problems, search,
    status, tabs,
};
use eframe::egui::{
    self, Color32, Context, FontId, Id, Label, Rect, RichText, Sense, UiBuilder, Vec2,
    ViewportCommand,
};
use rustode_workspace::bench;
use rustode_workspace::{
    Document, DocumentId, Error, Event, ExtensionEvent, GroupId, NOT_LOADED, Problem, Range,
    SearchEvent, ServerState, Workspace,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

mod overlays;

/// The status bar prompt after closing a tab with unsaved changes.
const CLOSE_PROMPT: &str = "Unsaved changes: Ctrl+W again closes without saving, Ctrl+S saves";
/// The status bar prompt after quitting with unsaved changes, as in the TUI.
const QUIT_PROMPT: &str = "Unsaved changes: Ctrl+Q again quits without saving, Ctrl+S saves";
/// How long one frame works on a running replace, so the window keeps
/// drawing.
const REPLACE_SLICE: Duration = Duration::from_millis(8);
const EDITOR_BACKGROUND: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
/// `editorGroup.border`, #FFFFFF17 over the editor background
/// (`dark_modern.json:36`).
const DIVIDER: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
const HINT: Color32 = Color32::from_rgb(0x9d, 0x9d, 0x9d);

/// The editor window: open documents in tabs and an optional folder in the
/// explorer.
pub struct EditorApp {
    workspace: Workspace,
    /// Where each document is scrolled to in each group, kept when
    /// switching tabs: every view of a document scrolls on its own.
    views: HashMap<(GroupId, DocumentId), editor::View>,
    /// The hover and suggest overlays' state.
    overlay: crate::suggest::State,
    explorer: explorer::View,
    strips: HashMap<GroupId, tabs::Strip>,
    /// The Ctrl+K chords of the group commands.
    group_chord: groups::Chord,
    /// Whether the sidebar is shown (Ctrl+B), and how wide.
    sidebar: layout::Sidebar,
    /// Where the sidebar was drawn last frame, to tell which part has focus.
    sidebar_area: Option<Rect>,
    /// The same for the problems panel.
    problems_area: Option<Rect>,
    /// Which status bar item has focus.
    status: status::Bar,
    /// A prompt or a failure of the last command; the next key clears it.
    message: Option<String>,
    /// Why the last save or edit failed; shown until a save succeeds.
    error: Option<String>,
    /// The latest notice about a file on disk.
    notice: Option<String>,
    /// The tab whose unsaved changes the next close discards.
    confirm_close: Option<DocumentId>,
    /// The title last sent to the window.
    title: Option<String>,
    /// Set when the window runs under [`crate::run`], to time frames.
    latency: Option<Rc<RefCell<PresentLatency>>>,
    /// The Extensions view, shown in the sidebar instead of the explorer.
    extensions: extensions::Panel,
    /// Whether the workspace wakes this window when language server
    /// messages or extension results arrive.
    waker_set: bool,
    /// The problems panel below the editor.
    problems: problems::Panel,
    /// The debug panel below the editor.
    debug: debug::Panel,
    /// Where the debug panel was drawn last frame.
    debug_area: Option<Rect>,
    /// Whether the editor takes focus on the next frame: after going to a
    /// problem with Enter, so the key does not reach the editor too.
    focus_editor_next: bool,
    /// The Search view, shown in the sidebar instead of the explorer.
    search: search::Panel,
    /// The open quick input, if any.
    quick: Option<QuickInput>,
    /// The find widget; kept while closed, so F3 repeats the last search.
    find: find::Find,
    /// Whether the next quit discards unsaved changes.
    confirm_quit: bool,
    /// The "Change Language Mode" picker.
    language: language::Picker,
    /// The list of locations and the rename box.
    navigator: navigate::Navigator,
    /// The benchmark script the window runs on itself, if one was asked for
    /// (`RUSTODE_BENCH_SCRIPT`); shared with [`crate::run`], which tells it
    /// when each frame was presented.
    bench: Option<Rc<RefCell<bench::Driver>>>,
    /// The report line the benchmark script ended with, once it has.
    bench_report: Option<String>,
}

fn file_name(document: &Document) -> String {
    document
        .path()
        .file_name()
        .map_or_else(Default::default, |name| name.to_string_lossy().into_owned())
}

/// Whether the explorer or the Extensions view's search box has focus.
fn sidebar_has_focus(ctx: &Context) -> bool {
    ctx.memory(|memory| {
        memory.has_focus(explorer::id()) || memory.has_focus(extensions::search_id())
    }) || search::has_focus(ctx)
}

/// Gives keyboard focus to `id`, and draws once more so the widget can lock
/// its keys before the next one arrives.
fn focus(ctx: &Context, id: egui::Id) {
    ctx.memory_mut(|memory| memory.request_focus(id));
    ctx.request_repaint();
}

impl EditorApp {
    /// Opens a file in a tab, or a folder in the explorer.
    ///
    /// # Errors
    /// When the path cannot be read.
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::open_paths(&[path.to_owned()])
    }

    /// Opens files in tabs and the first folder in the explorer, as the TUI
    /// does.
    ///
    /// # Errors
    /// When a path cannot be read.
    pub fn open_paths(paths: &[PathBuf]) -> Result<Self, Error> {
        Self::open_paths_in(Workspace::new(), paths)
    }

    /// [`Self::open_paths`] in a workspace set up by the caller, for
    /// example with its extensions folder.
    ///
    /// # Errors
    /// When a path cannot be read.
    pub fn open_paths_in(mut workspace: Workspace, paths: &[PathBuf]) -> Result<Self, Error> {
        for path in paths {
            if path.is_dir() {
                if workspace.explorer().is_none() {
                    workspace.open_folder(path)?;
                }
            } else {
                workspace.open(path)?;
            }
        }
        Ok(Self::new(workspace))
    }

    /// The window for `workspace` and the documents already open in it.
    #[must_use]
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            views: HashMap::new(),
            overlay: crate::suggest::State::default(),
            explorer: explorer::View::default(),
            strips: HashMap::new(),
            group_chord: groups::Chord::default(),
            sidebar: layout::Sidebar::default(),
            sidebar_area: None,
            problems_area: None,
            status: status::Bar::default(),
            message: None,
            error: None,
            notice: None,
            confirm_close: None,
            title: None,
            latency: None,
            extensions: extensions::Panel::default(),
            waker_set: false,
            search: search::Panel::default(),
            language: language::Picker::default(),
            navigator: navigate::Navigator::default(),
            problems: problems::Panel::default(),
            debug: debug::Panel::default(),
            debug_area: None,
            focus_editor_next: false,
            quick: None,
            find: find::Find::default(),
            confirm_quit: false,
            bench: None,
            bench_report: None,
        }
    }

    /// Has the window run `script` on itself, one step per frame, once its
    /// first frame was presented: the workspace's benchmark hook
    /// ([`rustode_workspace::bench`]). The report line goes to stderr.
    pub fn run_bench_script(&mut self, script: bench::Script) {
        self.bench = Some(Rc::new(RefCell::new(bench::Driver::new(script))));
    }

    /// The benchmark driver, for [`crate::run`] to time each presented frame
    /// (and for a test to play the part of the window system).
    #[must_use]
    pub fn bench_driver(&self) -> Option<Rc<RefCell<bench::Driver>>> {
        self.bench.clone()
    }

    /// The line the benchmark script reported, once it is over.
    #[must_use]
    pub fn bench_report(&self) -> Option<&str> {
        self.bench_report.as_deref()
    }

    /// One step of the benchmark script, if there is one: the workspace's
    /// driver decides, the window starts a search the way its Search view
    /// does and prints the report.
    fn bench_step(&mut self, ctx: &Context) {
        let Some(driver) = self.bench.clone() else {
            return;
        };
        let step = driver.borrow_mut().before_frame(&mut self.workspace);
        match step {
            bench::Step::Wait => ctx.request_repaint(),
            bench::Step::StartSearch(script) => {
                self.search.run_script(&mut self.workspace, &script);
                ctx.request_repaint();
            }
            bench::Step::Report(line) => {
                eprintln!("{line}");
                self.bench_report = Some(line);
                ctx.request_repaint();
            }
            bench::Step::Finished => {}
        }
    }

    /// Reads `settings.json` and `keybindings.json` from `dir` (VS Code's
    /// `User` folder layout), which starts the language servers they
    /// configure; problems in them show in the status bar.
    pub fn load_user_config(&mut self, dir: &Path) {
        let _ = self
            .workspace
            .load_config(&dir.join("settings.json"), &dir.join("keybindings.json"));
        let problems = self.workspace.config().diagnostics().count()
            + self.workspace.language_server_problems().len()
            + self.workspace.debug_problems().len();
        if problems > 0 {
            self.notice = Some(format!(
                "{problems} problem(s) in settings.json or keybindings.json"
            ));
        }
    }

    /// The workspace holding the documents.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// The workspace holding the documents, for the settings made once at
    /// startup (the extension folders, the built-in language servers).
    pub fn workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspace
    }

    /// The window title: `● ` while the active document has unsaved
    /// changes, then its file name, the folder's name and the app name.
    #[must_use]
    pub fn window_title(&self) -> String {
        let active = self
            .workspace
            .active_document()
            .and_then(|id| self.workspace.document(id));
        let mark = if active.is_some_and(Document::is_dirty) {
            "\u{25cf} "
        } else {
            ""
        };
        let root = self
            .workspace
            .explorer()
            .and_then(|explorer| explorer.root().file_name())
            .map(|name| name.to_string_lossy().into_owned());
        let parts: Vec<String> = [active.map(file_name), root, Some("rustode".to_owned())]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect();
        format!("{mark}{}", parts.join(" - "))
    }

    /// Reports each frame to `latency`, telling whether its input changed
    /// the window; [`crate::run`] does this for its window.
    pub fn time_frames(&mut self, latency: Rc<RefCell<PresentLatency>>) {
        self.latency = Some(latency);
    }

    /// Turns workspace events about files into status bar notices.
    fn note(&mut self) {
        for event in self.workspace.take_events() {
            self.notice = match event {
                Event::Conflict { .. } => {
                    Some("The file changed on disk; the unsaved changes are kept.".to_owned())
                }
                Event::Deleted { .. } => Some("The file was deleted from disk.".to_owned()),
                Event::Renamed { to, .. } => {
                    Some(format!("The file was moved to {}.", to.display()))
                }
                Event::Restored { .. } | Event::Reloaded { .. } => None,
                Event::Notice(text) => Some(text),
                Event::Navigated { id } => {
                    self.view_of(id).reveal();
                    continue;
                }
                Event::Search(SearchEvent::Replaced) => {
                    self.notice = self.search.replaced(&mut self.workspace);
                    continue;
                }
                Event::LocationsChanged => {
                    if let Some(text) = self.navigator.locations_changed(&self.workspace) {
                        self.notice = Some(text);
                    }
                    continue;
                }
                Event::Extension(event) => match extension_notice(&event) {
                    Some(notice) => Some(notice),
                    None => continue,
                },
                Event::LanguageServerChanged {
                    language,
                    state: ServerState::Crashed,
                } => Some(self.server_notice(&language)),
                _ => continue,
            };
        }
    }

    /// What the crashed server of `language` says about itself.
    fn server_notice(&self, language: &str) -> String {
        let why = self
            .workspace
            .language_servers()
            .into_iter()
            .find_map(|s| s.message.filter(|_| s.language == language))
            .unwrap_or_else(|| "crashed".to_owned());
        format!("Language server for {language}: {why}")
    }

    /// Asks the workspace to repaint the window when language server
    /// messages or extension results arrive from its background threads.
    fn wake_on_results(&mut self, ctx: &Context) {
        if !self.waker_set {
            let ctx = ctx.clone();
            self.workspace.set_waker(move || ctx.request_repaint());
            self.waker_set = true;
        }
    }

    /// Ctrl+Shift+M: shows the panel and focuses its first row, or hides it
    /// when one of its rows has focus.
    fn toggle_problems(&mut self, ctx: &Context) {
        let count = self.workspace.problems().len();
        if self.problems.visible && problems::has_focus(ctx, count) {
            self.problems.visible = false;
            self.focus_editor(ctx);
        } else {
            self.problems.visible = true;
            self.problems.focus_first = true;
            ctx.request_repaint();
        }
    }

    /// Opens or activates the problem's tab and selects the problem; the
    /// editor takes focus on the next frame.
    fn go_to_problem(&mut self, ctx: &Context, problem: &Problem) {
        match self.workspace.go_to_problem(problem) {
            Ok(id) => {
                self.view_of(id).reveal();
                self.focus_editor_next = true;
                ctx.request_repaint();
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    /// The debug panel below the editor, if shown; returns the rest.
    fn show_debug(&mut self, ui: &mut egui::Ui, rect: Rect) -> Rect {
        if !debug::visible(&self.debug, &self.workspace) {
            self.debug_area = None;
            return rect;
        }
        let height = debug::PANEL_HEIGHT.min(rect.height() / 2.0);
        let (above, lower) = rect.split_top_bottom_at_y(rect.bottom() - height);
        self.debug_area = Some(lower);
        let outcome = ui
            .scope_builder(
                UiBuilder::new().max_rect(lower).id_salt("rustode-debug"),
                |ui| debug::show(ui, &mut self.workspace, &mut self.debug),
            )
            .inner;
        if let Some(error) = outcome.error {
            self.message = Some(error);
        }
        above
    }

    /// The panel below the editor, if shown; returns the editor's rect.
    fn show_problems(&mut self, ui: &mut egui::Ui, rect: Rect) -> Rect {
        if !self.problems.visible {
            self.problems_area = None;
            return rect;
        }
        let height = problems::PANEL_HEIGHT.min(rect.height() / 2.0);
        let (editor, panel) = rect.split_top_bottom_at_y(rect.bottom() - height);
        self.problems_area = Some(panel);
        let chosen = ui
            .scope_builder(
                UiBuilder::new().max_rect(panel).id_salt("rustode-problems"),
                |ui| problems::show(ui, &self.workspace),
            )
            .inner;
        if std::mem::take(&mut self.problems.focus_first) {
            focus(ui.ctx(), problems::first_row_id());
        }
        if let Some(problem) = chosen {
            self.go_to_problem(ui.ctx(), &problem);
        }
        editor
    }

    fn update_title(&mut self, ctx: &Context) {
        let title = self.window_title();
        if self.title.as_ref() != Some(&title) {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = Some(title);
        }
    }

    /// Takes the workbench keys out of this frame's input and runs them.
    /// Any other key clears the last message and the pending close.
    fn workbench_keys(&mut self, ctx: &Context) -> bool {
        let (commands, typed) = ctx.input_mut(|input| {
            let mut commands = Vec::new();
            input.events.retain(|event| match keys::workbench(event) {
                Some(command) => {
                    commands.push(command);
                    false
                }
                None => true,
            });
            let typed = input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key { pressed: true, .. } | egui::Event::Text(_)
                )
            });
            (commands, typed)
        });
        if typed || !commands.is_empty() {
            self.message = None;
        }
        if typed
            || commands
                .iter()
                .any(|&command| command != Workbench::CloseTab)
        {
            self.confirm_close = None;
        }
        if typed || commands.iter().any(|&command| command != Workbench::Quit) {
            self.confirm_quit = false;
        }
        for &command in &commands {
            self.run_workbench(ctx, command);
        }
        !commands.is_empty()
    }

    fn run_workbench(&mut self, ctx: &Context, command: Workbench) {
        match command {
            Workbench::Save => self.save(),
            Workbench::NextTab => self.switch(ctx, Workspace::next_tab),
            Workbench::PreviousTab => self.switch(ctx, Workspace::previous_tab),
            Workbench::RecentTab => self.switch(ctx, Workspace::recently_used_tab),
            Workbench::LeastRecentTab => self.switch(ctx, Workspace::least_recently_used_tab),
            Workbench::CloseTab => self.close_active(ctx),
            Workbench::Group(id) => self.run_group_command(ctx, id),
            Workbench::ToggleSidebar => self.toggle_sidebar(ctx),
            Workbench::FocusExplorer => self.focus_explorer(ctx),
            Workbench::FocusNextPart
                if self
                    .workspace
                    .command_applies(rustode_workspace::PAUSE_COMMAND) =>
            {
                self.debug_key(ctx, rustode_workspace::PAUSE_COMMAND);
            }
            Workbench::FocusNextPart => self.focus_part(ctx, true),
            Workbench::FocusPreviousPart => self.focus_part(ctx, false),
            Workbench::ToggleTabMovesFocus => focus::toggle_tab_moves_focus(ctx),
            Workbench::ShowExtensions => self.show_extensions(ctx),
            Workbench::ToggleProblems => self.toggle_problems(ctx),
            Workbench::ShowSearch => {
                self.sidebar.reveal();
                self.extensions.shown = false;
                self.search.show_search(&self.workspace);
            }
            Workbench::ShowReplace => {
                self.sidebar.reveal();
                self.extensions.shown = false;
                self.search.show_replace(&self.workspace);
            }
            Workbench::SearchReplaceAll => {
                self.message = self.search.ask_replace_all(&self.workspace);
            }
            Workbench::SearchReplace => {
                self.message = self
                    .search
                    .run_selected(&mut self.workspace, search::Act::Replace);
            }
            Workbench::SearchReplaceFile => {
                self.message = self
                    .search
                    .run_selected(&mut self.workspace, search::Act::ReplaceFile);
            }
            Workbench::SearchRemove => {
                self.message = self
                    .search
                    .run_selected(&mut self.workspace, search::Act::Remove);
            }
            Workbench::SearchUndoReplace => {
                self.message = self
                    .search
                    .run_selected(&mut self.workspace, search::Act::Undo);
            }
            Workbench::Quit => self.quit(ctx),
            Workbench::QuickOpen => self.open_quick(ctx, ""),
            Workbench::ShowCommands => self.open_quick(ctx, ">"),
            Workbench::GotoLine => self.open_quick(ctx, ":"),
            Workbench::DebugKey
            | Workbench::DebugStop
            | Workbench::ToggleBreakpoint
            | Workbench::StepOver
            | Workbench::StepInto
            | Workbench::StepOut
            | Workbench::FocusCallStack
            | Workbench::FocusVariables
            | Workbench::FocusWatch
            | Workbench::FocusConsole
            | Workbench::RemoveWatch => self.run_debug_workbench(ctx, command),
            Workbench::Find
            | Workbench::Replace
            | Workbench::FindNext
            | Workbench::FindPrevious => self.run_find(ctx, command),
        }
    }

    /// Ctrl+W: closes the active editor, or an empty group.
    fn close_active(&mut self, ctx: &Context) {
        if let Some(id) = self.workspace.active_document() {
            self.close(ctx, id);
        } else if self
            .workspace
            .command_applies("workbench.action.closeGroup")
        {
            self.run_group_command(ctx, "workbench.action.closeGroup");
        }
    }

    /// The steps and the debug panel's commands. F6 pauses while the program
    /// runs (`debugState == 'running'` wins over moving the focus, as in VS
    /// Code); see `Workbench::FocusNextPart`.
    fn run_debug_workbench(&mut self, ctx: &Context, command: Workbench) {
        match command {
            Workbench::DebugKey => {
                let id = self.workspace.debug_key_command();
                self.run_command(ctx, id);
            }
            Workbench::DebugStop => self.run_command(ctx, "workbench.action.debug.stop"),
            Workbench::ToggleBreakpoint => {
                self.run_command(ctx, "editor.debug.action.toggleBreakpoint");
            }
            Workbench::StepOver => self.debug_key(ctx, rustode_workspace::STEP_OVER_COMMAND),
            Workbench::StepInto => self.debug_key(ctx, rustode_workspace::STEP_INTO_COMMAND),
            Workbench::StepOut => self.debug_key(ctx, rustode_workspace::STEP_OUT_COMMAND),
            Workbench::FocusCallStack => self.debug.focus(debug::Column::CallStack),
            Workbench::FocusVariables => self.debug.focus(debug::Column::Variables),
            Workbench::FocusWatch => self.debug.focus(debug::Column::Watch),
            Workbench::FocusConsole => self.debug.focus(debug::Column::Console),
            Workbench::RemoveWatch => self.remove_watch(),
            _ => {}
        }
    }

    fn remove_watch(&mut self) {
        if !self.debug.remove_focused_watch(&mut self.workspace) {
            self.message = Some("Select a watch expression first".to_owned());
        }
    }

    /// A debug key whose `when` clause decides if it means anything now.
    fn debug_key(&mut self, ctx: &Context, id: &str) {
        if self.workspace.command_applies(id) {
            self.run_command(ctx, id);
        }
    }

    /// A tab switch, which also focuses the editor.
    fn switch(&mut self, ctx: &Context, switch: fn(&mut Workspace)) {
        switch(&mut self.workspace);
        self.focus_editor(ctx);
    }

    /// "Change Language Mode": Ctrl+K M or a click on the status bar's
    /// language (`open`) opens the picker, which is drawn while it is open.
    /// Returns whether a pick changed the document's language.
    fn change_language(&mut self, ctx: &Context, open: bool) -> bool {
        if let Some(id) = self.workspace.active_document().filter(|_| open) {
            self.language.open(id);
        }
        match self.language.show(ctx, &mut self.workspace) {
            language::Outcome::Picked => {
                self.focus_editor(ctx);
                true
            }
            language::Outcome::Closed => {
                self.focus_editor(ctx);
                false
            }
            language::Outcome::Failed(message) => {
                self.error = Some(message);
                false
            }
            language::Outcome::None => false,
        }
    }

    /// Focuses the editor, or the empty editor group without a tab.
    fn focus_editor(&self, ctx: &Context) {
        if self.workspace.active_document().is_some() {
            focus(ctx, editor::id());
        } else {
            focus(ctx, focus::empty_group_id());
        }
    }

    fn save(&mut self) {
        let Some(id) = self.workspace.active_document() else {
            return;
        };
        match self.workspace.save(id) {
            Ok(()) => self.error = None,
            Err(err) => self.error = Some(format!("Save failed: {err}")),
        }
    }

    /// Closes `id`'s tab; with unsaved changes, only when asked twice.
    fn close(&mut self, ctx: &Context, id: DocumentId) {
        let discard = self.confirm_close == Some(id);
        let group = self.workspace.active_group();
        match self.workspace.close_editor(group, id, discard) {
            Ok(()) => {
                self.confirm_close = None;
                self.message = None;
                self.views.remove(&(group, id));
                if self.workspace.active_document().is_some() {
                    self.focus_editor(ctx);
                } else if self.sidebar.visible() && self.workspace.explorer().is_some() {
                    focus(ctx, explorer::id());
                } else {
                    self.focus_editor(ctx);
                }
            }
            Err(Error::Dirty { .. }) => {
                self.activate(id);
                self.confirm_close = Some(id);
                self.message = Some(CLOSE_PROMPT.to_owned());
            }
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    fn toggle_sidebar(&mut self, ctx: &Context) {
        self.sidebar.toggle();
        if !self.sidebar.visible() && sidebar_has_focus(ctx) {
            self.focus_editor(ctx);
        }
    }

    /// Ctrl+Shift+X: shows the sidebar with the Extensions view and focuses
    /// its search box, or, from the search box, goes back to the editor.
    fn show_extensions(&mut self, ctx: &Context) {
        self.sidebar.reveal();
        self.search.shown = false;
        self.notice = self.extensions.toggle(ctx, &mut self.workspace);
    }

    /// Ctrl+Shift+E: shows the sidebar and focuses the explorer on the
    /// active file's row.
    fn focus_explorer(&mut self, ctx: &Context) {
        self.sidebar.reveal();
        self.extensions.shown = false;
        self.search.shown = false;
        if self.workspace.explorer().is_none() {
            self.message = Some("No folder opened".to_owned());
            return;
        }
        let active = self
            .workspace
            .active_document()
            .and_then(|id| self.workspace.document(id))
            .map(|document| document.path().to_owned());
        if let Some(path) = active {
            self.explorer.select(&self.workspace, &path);
        }
        focus(ctx, explorer::id());
    }

    /// F6 (`next`) and Shift+F6: the next or the previous part, in VS
    /// Code's order, skipping the parts not shown.
    fn focus_part(&mut self, ctx: &Context, next: bool) {
        let sidebar = self.sidebar.visible()
            && (self.extensions.shown || self.search.shown || self.workspace.explorer().is_some());
        let debug = debug::visible(&self.debug, &self.workspace);
        let problems = self.problems.visible && !self.workspace.problems().is_empty();
        let panel = problems || debug;
        let panels = match (self.problems_area, self.debug_area) {
            (Some(a), Some(b)) => Some(a.union(b)),
            (a, b) => a.or(b),
        };
        let status = ctx.content_rect().height() >= layout::STATUS_BAR_BREAKPOINT;
        let target = match focus::focused_part(ctx, self.sidebar_area, panels) {
            Some(part) => focus::neighbour(part, next, |part| match part {
                Part::Sidebar => sidebar,
                Part::Panel => panel,
                Part::StatusBar => status,
                Part::Editor => true,
            }),
            None => Part::Editor,
        };
        match target {
            Part::Editor => self.focus_editor(ctx),
            Part::Panel if problems => focus(ctx, problems::first_row_id()),
            Part::Panel => focus(ctx, debug::console_id()),
            Part::StatusBar => focus(ctx, status::id()),
            Part::Sidebar if self.search.shown => {
                self.search.focus();
                ctx.request_repaint();
            }
            Part::Sidebar if self.extensions.shown => focus(ctx, extensions::search_id()),
            Part::Sidebar => self.focus_explorer(ctx),
        }
    }

    /// With nothing focused, the editor takes focus, or else the explorer,
    /// or else the empty editor group.
    fn default_focus(&self, ctx: &Context) {
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        if self.workspace.active_document().is_none()
            && self.sidebar.visible()
            && self.workspace.explorer().is_some()
        {
            focus(ctx, explorer::id());
        } else {
            self.focus_editor(ctx);
        }
    }

    fn show_sidebar(&mut self, ui: &mut egui::Ui, rect: Rect) -> bool {
        if self.search.shown {
            let outcome = ui
                .scope_builder(
                    UiBuilder::new().max_rect(rect).id_salt("rustode-sidebar"),
                    |ui| {
                        ui.set_clip_rect(rect);
                        search::show(ui, &mut self.workspace, &mut self.search)
                    },
                )
                .inner;
            if let Some(message) = outcome.message {
                self.message = Some(message);
            }
            if outcome.focus_editor {
                self.focus_editor(ui.ctx());
            }
            return false;
        }
        if self.extensions.shown {
            let refused = ui
                .scope_builder(
                    UiBuilder::new().max_rect(rect).id_salt("rustode-sidebar"),
                    |ui| {
                        ui.set_clip_rect(rect);
                        extensions::show(ui, &mut self.workspace, &mut self.extensions)
                    },
                )
                .inner;
            if refused.is_some() {
                self.notice = refused;
            }
            return false;
        }
        let outcome = ui
            .scope_builder(
                UiBuilder::new().max_rect(rect).id_salt("rustode-sidebar"),
                |ui| explorer::show(ui, &mut self.workspace, &mut self.explorer),
            )
            .inner;
        if let Some(message) = outcome.message {
            self.message = Some(message);
        }
        if outcome.focus_editor {
            self.focus_editor(ui.ctx());
        }
        outcome.changed
    }

    /// One tab strip per group.
    fn show_tabs(&mut self, ui: &mut egui::Ui, placed: &groups::Arranged) -> bool {
        let mut changed = false;
        for group in &placed.groups {
            let strip = self.strips.entry(group.group).or_default();
            let action = ui
                .scope_builder(
                    UiBuilder::new()
                        .max_rect(group.tabs)
                        .id_salt(("rustode-tabs", group.group)),
                    |ui| tabs::show(ui, &self.workspace, group.group, strip),
                )
                .inner;
            changed |= self.tab_action(ui.ctx(), group.group, action);
        }
        changed
    }

    /// Switches to a tab; why it could not is shown in the status bar.
    fn activate(&mut self, id: DocumentId) {
        if let Err(err) = self.workspace.activate(id) {
            self.message = Some(err.to_string());
        }
    }

    /// Gives a group the focus; why it could not is shown in the status bar.
    fn focus_group(&mut self, group: GroupId) {
        if let Err(err) = self.workspace.focus_group(group) {
            self.message = Some(err.to_string());
        }
    }

    /// What a click or key on a group's strip asked. A click on a group
    /// that does not have the focus gives it the focus first.
    fn tab_action(&mut self, ctx: &Context, group: GroupId, action: Option<tabs::Action>) -> bool {
        let Some(action) = action else { return false };
        if group != self.workspace.active_group() {
            self.focus_group(group);
        }
        match action {
            tabs::Action::Activate(id) => {
                self.activate(id);
                self.focus_editor(ctx);
            }
            tabs::Action::Select(id) => {
                self.activate(id);
                focus(ctx, tabs::tab_id(id));
            }
            tabs::Action::Close(id) => self.close(ctx, id),
        }
        true
    }

    /// The scroll state of the active group's view of `id`.
    pub(super) fn view_of(&mut self, id: DocumentId) -> &mut editor::View {
        let group = self.workspace.active_group();
        self.views.entry((group, id)).or_default()
    }

    /// Runs a group command by id; the focus follows into the editor.
    pub(super) fn run_group_command(&mut self, ctx: &Context, id: &str) {
        match self.workspace.execute(id) {
            Ok(_) => {
                self.message = None;
                self.focus_editor(ctx);
            }
            Err(Error::Dirty { .. }) => {
                self.message = Some(CLOSE_PROMPT.to_owned());
            }
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    /// The editors of every group, and the dividers between them.
    fn show_editors(&mut self, ui: &mut egui::Ui, placed: &groups::Arranged, bottom: f32) -> bool {
        let mut changed = false;
        for divider in &placed.dividers {
            let rect = Rect::from_min_max(
                divider.min,
                egui::pos2(divider.max.x, divider.max.y.min(bottom)),
            );
            ui.painter().rect_filled(rect, 0.0, DIVIDER);
        }
        for group in &placed.groups {
            let mut rect = group.editor;
            rect.max.y = rect.max.y.min(bottom).max(rect.min.y);
            changed |= self.show_group_editor(ui, group.group, rect);
        }
        changed
    }

    fn show_group_editor(&mut self, ui: &mut egui::Ui, group: GroupId, rect: Rect) -> bool {
        let active = group == self.workspace.active_group();
        let shown = self
            .workspace
            .group_by_id(group)
            .and_then(rustode_workspace::EditorGroup::active);
        let Some(id) = shown else {
            empty_group(ui, rect);
            if active {
                focus::empty_group(ui, rect);
            } else if ui
                .interact(
                    rect,
                    Id::new(("rustode-empty-group", group)),
                    Sense::click(),
                )
                .clicked()
            {
                let _ = self.workspace.focus_group(group);
                self.focus_editor(ui.ctx());
            }
            return false;
        };
        let name = self
            .workspace
            .document(id)
            .map_or_else(Default::default, file_name);
        let view = self.views.entry((group, id)).or_default();
        let matches: &[Range] = if active {
            self.find.highlights(&self.workspace)
        } else {
            &[]
        };
        let workspace = &mut self.workspace;
        let overlay = &mut self.overlay;
        let outcome = ui
            .scope_builder(
                UiBuilder::new()
                    .max_rect(rect)
                    .id_salt(("rustode-editor-area", group)),
                |ui| {
                    editor::show(
                        ui,
                        workspace,
                        id,
                        view,
                        overlay,
                        &editor::Shown {
                            label: &name,
                            matches,
                            group,
                            active,
                        },
                    )
                },
            )
            .inner;
        if let Some(error) = outcome.error {
            self.error = Some(error);
        }
        if let Some(offset) = outcome.pressed {
            // A press on another group focuses it and puts its caret there.
            self.focus_group(group);
            if let Err(err) = self.workspace.set_cursor(id, offset) {
                self.message = Some(err.to_string());
            }
            self.focus_editor(ui.ctx());
            return true;
        }
        outcome.changed
    }
}

/// The editor area with no tab open: how to open one, as in the TUI.
fn empty_group(ui: &mut egui::Ui, rect: Rect) {
    ui.painter().rect_filled(rect, 0.0, EDITOR_BACKGROUND);
    let lines = [
        "No file open",
        "Open one with Ctrl+P, or from the explorer: Ctrl+Shift+E or Shift+F6",
    ];
    for (row, line) in (0_u8..).zip(lines) {
        let center = rect.center() + Vec2::new(0.0, (f32::from(row) - 0.5) * 20.0);
        let place = Rect::from_center_size(center, Vec2::new(rect.width(), 20.0));
        let text = RichText::new(line)
            .font(FontId::proportional(13.0))
            .color(HINT);
        ui.put(place, Label::new(text).selectable(false).truncate());
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.wake_on_results(&ctx);
        if std::mem::take(&mut self.focus_editor_next) {
            self.focus_editor(&ctx);
        }
        self.workspace.poll_external_changes();
        self.workspace.poll_extensions();
        self.workspace.poll_language_servers();
        self.workspace.poll_debug();
        self.workspace.poll_search();
        if self.workspace.poll_replace(Some(REPLACE_SLICE)) {
            ctx.request_repaint();
        }
        self.bench_step(&ctx);
        self.note();
        let tab = focus::tab_pressed(&ctx);
        // Before the editor, which also reads Ctrl+K for Ctrl+K Ctrl+I.
        let chord = !self.language.is_open() && self.language.chord(&ctx);
        let mut changed = self.workbench_keys(&ctx);
        for id in self.group_chord.take(&ctx) {
            self.run_group_command(&ctx, id);
            changed = true;
        }
        self.default_focus(&ctx);
        let rect = ui.max_rect();
        let areas = layout::areas(rect, self.sidebar.fit(rect.width()));
        self.sidebar_area = areas.sidebar;
        changed |= self.show_overlays(ui, &areas);
        if let Some(sidebar) = areas.sidebar {
            changed |= self.show_sidebar(ui, sidebar);
        }
        let placed = groups::arrange(
            self.workspace.layout(),
            areas.tabs.union(areas.editor),
            self.workspace.active_group(),
        );
        changed |= self.show_tabs(ui, &placed);
        let above = self.show_debug(ui, areas.editor);
        let editor = self.show_problems(ui, above);
        changed |= self.show_editors(ui, &placed, editor.bottom());
        if self.find.is_stale(&self.workspace) {
            // The editor changed the text after the matches were found.
            ctx.request_repaint();
        }
        self.note();
        let message = self
            .message
            .as_deref()
            .or(self.error.as_deref())
            .or(self.notice.as_deref());
        let active = self.workspace.active_document();
        let mut clicked_language = false;
        if let Some(rect) = areas.status {
            let outcome = ui
                .scope_builder(
                    UiBuilder::new()
                        .max_rect(rect)
                        .id(status::id())
                        .sense(Sense::FOCUSABLE),
                    |ui| status::show(ui, &self.workspace, active, message, &mut self.status),
                )
                .inner;
            if outcome.toggle_problems {
                self.toggle_problems(&ctx);
            }
            clicked_language = outcome.language;
            if outcome.leave {
                self.focus_editor(&ctx);
            }
        }
        if tab {
            // egui moves focus on Tab at the end of a frame, or on the next
            // one when it wraps around: draw that one now.
            ctx.request_repaint();
        }
        changed |= self.change_language(&ctx, chord || clicked_language);
        self.update_title(&ctx);
        if let Some(latency) = &self.latency {
            latency.borrow_mut().frame(changed);
        }
    }
}

/// The status bar's notice for an extension event, if it has one.
fn extension_notice(event: &ExtensionEvent) -> Option<String> {
    Some(match event {
        ExtensionEvent::Installed { id, version } => {
            format!("Installed {id} {version} ({}).", NOT_LOADED.to_lowercase())
        }
        ExtensionEvent::Uninstalled { id } => format!("Uninstalled {id}."),
        ExtensionEvent::Enabled { id } => {
            format!("Enabled {id} ({}).", NOT_LOADED.to_lowercase())
        }
        ExtensionEvent::Disabled { id } => format!("Disabled {id}."),
        ExtensionEvent::Failed { id, message } => format!("{id}: {message}"),
        ExtensionEvent::ListFailed { message } | ExtensionEvent::StateFailed { message } => {
            message.clone()
        }
        _ => return None,
    })
}
