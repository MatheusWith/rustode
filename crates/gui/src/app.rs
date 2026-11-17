//! The window: the editor above the status bar, and the title.
//!
//! The title follows VS Code's default (`${dirty}${activeEditorShort} - …`,
//! with `● ` for unsaved changes), studied in
//! `src/vs/workbench/browser/parts/titlebar/windowTitle.ts:41-53`, `:59`.

use crate::latency::PresentLatency;
use crate::status::STATUS_BAR_HEIGHT;
use crate::{editor, status};
use eframe::egui::{self, UiBuilder, ViewportCommand};
use rustode_workspace::{DocumentId, Event, Workspace};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// The editor window for one document.
pub struct EditorApp {
    workspace: Workspace,
    document: DocumentId,
    /// The file name, shown in the title and given to screen readers.
    name: String,
    view: editor::View,
    /// Why the last save failed; shown until a save succeeds.
    error: Option<String>,
    /// The latest notice about the file on disk.
    notice: Option<String>,
    /// The title last sent to the window.
    title: Option<String>,
    /// Set when the window runs under [`crate::run`], to time frames.
    latency: Option<Rc<RefCell<PresentLatency>>>,
}

impl EditorApp {
    /// Opens `path` in a new workspace.
    ///
    /// # Errors
    /// When the file cannot be read.
    pub fn open(path: &Path) -> Result<Self, rustode_workspace::Error> {
        let mut workspace = Workspace::new();
        let document = workspace.open(path)?;
        Ok(Self::new(workspace, document))
    }

    /// The window for `document`, already open in `workspace`.
    #[must_use]
    pub fn new(workspace: Workspace, document: DocumentId) -> Self {
        let name = workspace
            .document(document)
            .and_then(|doc| doc.path().file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            workspace,
            document,
            name,
            view: editor::View::default(),
            error: None,
            notice: None,
            title: None,
            latency: None,
        }
    }

    /// The window title: `● ` while there are unsaved changes, then the
    /// file name and the app name.
    #[must_use]
    pub fn window_title(&self) -> String {
        let dirty = self
            .workspace
            .document(self.document)
            .is_some_and(rustode_workspace::Document::is_dirty);
        let mark = if dirty { "\u{25cf} " } else { "" };
        format!("{mark}{} - rustode", self.name)
    }

    pub(crate) fn time_frames(&mut self, latency: Rc<RefCell<PresentLatency>>) {
        self.latency = Some(latency);
    }

    /// Turns workspace events about the file into status bar notices.
    fn note(&mut self, events: Vec<Event>) {
        for event in events {
            self.notice = match event {
                Event::Conflict { .. } => {
                    Some("The file changed on disk; the unsaved changes are kept.".to_owned())
                }
                Event::Deleted { .. } => Some("The file was deleted from disk.".to_owned()),
                Event::Renamed { to, .. } => {
                    Some(format!("The file was moved to {}.", to.display()))
                }
                Event::Restored { .. } | Event::Reloaded { .. } => None,
                _ => continue,
            };
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = self.window_title();
        if self.title.as_ref() != Some(&title) {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = Some(title);
        }
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.workspace.poll_external_changes();
        let events = self.workspace.take_events();
        self.note(events);
        let (editor_rect, status_rect) = ui
            .max_rect()
            .split_top_bottom_at_y(ui.max_rect().bottom() - STATUS_BAR_HEIGHT);
        let outcome = ui
            .scope_builder(UiBuilder::new().max_rect(editor_rect), |ui| {
                editor::show(
                    ui,
                    &mut self.workspace,
                    self.document,
                    &mut self.view,
                    &self.name,
                )
            })
            .inner;
        if outcome.saved {
            self.error = None;
        }
        if let Some(error) = outcome.error {
            self.error = Some(error);
        }
        let events = self.workspace.take_events();
        self.note(events);
        let message = self.error.as_deref().or(self.notice.as_deref());
        ui.scope_builder(UiBuilder::new().max_rect(status_rect), |ui| {
            status::show(ui, &self.workspace, self.document, message);
        });
        self.update_title(ui.ctx());
        if let Some(latency) = &self.latency {
            latency.borrow_mut().frame(outcome.changed);
        }
    }
}
