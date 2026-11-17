//! The TUI's state and event handling.

use crate::keys::{self, Command, ExplorerKey};
use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::{DefaultTerminal, Frame, Terminal};
use rustode_workspace::trace::{LatencyRecorder, LatencyReport};
use rustode_workspace::{
    Document, DocumentId, Error, Event as WorkspaceEvent, ExplorerRow, Movement, Resolution,
    Workspace,
};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often the idle event loop checks the disk for external changes.
const DISK_POLL: Duration = Duration::from_millis(250);

/// Which part of the screen receives keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    Editor,
    Explorer,
}

/// Where a document's view is scrolled to.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Viewport {
    /// First visible line.
    pub top: usize,
    /// First visible display column.
    pub left: usize,
}

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
    /// Explorer rows that fit on screen, from the last frame.
    pub(crate) explorer_height: usize,
    /// One line for the status bar: a save result, an error or a prompt.
    pub(crate) message: Option<String>,
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
            explorer_height: 1,
            message: None,
            confirm_quit: false,
            confirm_close: None,
            quit: false,
            latency: LatencyRecorder::new(),
        })
    }

    /// The workspace holding the documents.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
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
    /// 250 ms to pick up changes made to files by other programs.
    ///
    /// # Errors
    /// When the terminal cannot be read or drawn.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        terminal.draw(|frame| self.draw(frame))?;
        while !self.quit {
            if event::poll(DISK_POLL)? {
                let event = event::read()?;
                self.step(terminal, &event)?;
            } else {
                self.workspace.poll_external_changes();
                if self.take_events() {
                    terminal.draw(|frame| self.draw(frame))?;
                }
            }
        }
        Ok(())
    }

    /// Applies one terminal event to the workspace.
    pub fn handle(&mut self, event: &Event) {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => match self.focus {
                Focus::Editor => {
                    if let Some(command) = keys::command(*key) {
                        self.run_command(command);
                    }
                }
                Focus::Explorer => {
                    if let Some(command) = keys::global(*key) {
                        self.run_command(command);
                    } else if let Some(explorer_key) = keys::explorer_key(*key) {
                        self.message = None;
                        self.explorer_key(explorer_key);
                    }
                }
            },
            Event::Paste(text) if self.focus == Focus::Editor => {
                self.confirm_quit = false;
                self.message = None;
                self.report(|ws, id| ws.type_text(id, text));
            }
            _ => {}
        }
        self.take_events();
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
            Command::Reload => self.reload(),
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
            Focus::Explorer if self.document().is_some() => self.focus = Focus::Editor,
            Focus::Explorer => {}
        }
    }

    pub(crate) fn rows(&self) -> Vec<ExplorerRow> {
        self.workspace
            .explorer()
            .map(rustode_workspace::Explorer::rows)
            .unwrap_or_default()
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
                _ => continue,
            };
            self.message = Some(message);
        }
        !events.is_empty()
    }
}
