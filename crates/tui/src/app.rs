//! The TUI's state and event handling.

use crate::keys::{self, Command};
use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::{DefaultTerminal, Frame, Terminal};
use rustode_workspace::trace::{LatencyRecorder, LatencyReport};
use rustode_workspace::{
    Document, DocumentId, Error, Event as WorkspaceEvent, Movement, Resolution, Workspace,
};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// How often the idle event loop checks the disk for external changes.
const DISK_POLL: Duration = Duration::from_millis(250);

/// The terminal editor: one document, its viewport, and the time from each
/// key to the frame that shows it.
#[derive(Debug)]
pub struct App {
    pub(crate) workspace: Workspace,
    pub(crate) document: DocumentId,
    /// First visible line.
    pub(crate) top: usize,
    /// First visible display column.
    pub(crate) left: usize,
    /// Lines Page Up/Down move: the editor's height minus 2, as in VS Code
    /// (`src/vs/editor/common/cursorCommon.ts:129`).
    pub(crate) page: usize,
    /// One line for the status bar: a save result, an error or a prompt.
    pub(crate) message: Option<String>,
    confirm_quit: bool,
    quit: bool,
    latency: LatencyRecorder,
}

impl App {
    /// Opens `path` in a new workspace.
    ///
    /// # Errors
    /// When the file cannot be read.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let mut workspace = Workspace::new();
        let document = workspace.open(path)?;
        Ok(Self {
            workspace,
            document,
            top: 0,
            left: 0,
            page: 1,
            message: None,
            confirm_quit: false,
            quit: false,
            latency: LatencyRecorder::new(),
        })
    }

    /// The workspace holding the document.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// The open document.
    #[must_use]
    pub fn document(&self) -> DocumentId {
        self.document
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
    /// 250 ms to pick up changes made to the file by other programs.
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
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if let Some(command) = keys::command(*key) {
                    self.run_command(command);
                }
            }
            Event::Paste(text) => {
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
        self.message = None;
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
            Command::Reload => self.reload(),
            Command::Save => self.save(),
            Command::Quit => self.quit_or_confirm(),
        }
    }

    /// Runs a workspace command and shows its error, if any.
    fn report(&mut self, f: impl FnOnce(&mut Workspace, DocumentId) -> Result<(), Error>) {
        if let Err(err) = f(&mut self.workspace, self.document) {
            self.message = Some(err.to_string());
        }
    }

    /// Ctrl+S. After the file changed on disk under unsaved edits, saving
    /// keeps the user's version (the conflict prompt says so).
    fn save(&mut self) {
        let id = self.document;
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
        let id = self.document;
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
            .document(self.document)
            .is_some_and(Document::is_dirty);
        if !dirty || self.confirm_quit {
            self.quit = true;
        } else {
            self.confirm_quit = true;
            self.message =
                Some("Unsaved changes: Ctrl+Q again quits without saving, Ctrl+S saves".to_owned());
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
