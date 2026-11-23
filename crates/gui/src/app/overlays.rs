//! The window's part in the quick input and the find widget: opening them
//! from the workbench keys, running the row the quick input chose (a file's
//! tab, a line, or a command of the workspace's registry, which the
//! workspace runs on the active document or, for a workbench command, the
//! window runs itself), and showing both above the workbench.

use super::{EditorApp, QUIT_PROMPT, focus};
use crate::groups;
use crate::keys::Workbench;
use crate::layout::Areas;
use crate::quick_input::{self, Accepted, Close, QuickInput};
use crate::{editor, explorer, find, navigate};
use eframe::egui::{self, Context, ViewportCommand};
use rustode_workspace::Document;

impl EditorApp {
    /// Ctrl+F, Ctrl+H, F3 and Shift+F3.
    pub(super) fn run_find(&mut self, ctx: &Context, command: Workbench) {
        let result = match command {
            Workbench::Find | Workbench::Replace => {
                self.find
                    .open(ctx, &mut self.workspace, command == Workbench::Replace)
            }
            _ => self
                .find
                .step(&mut self.workspace, command == Workbench::FindNext),
        };
        match result {
            Ok(true) => self.reveal(),
            Ok(false) => {}
            Err(err) => self.message = Some(err),
        }
    }

    /// Scrolls the active editor to its caret on the next frame.
    pub(super) fn reveal(&mut self) {
        if let Some(id) = self.workspace.active_document() {
            self.view_of(id).reveal();
        }
    }

    /// Ctrl+Q: closes the window; with unsaved changes, only when asked twice.
    pub(super) fn quit(&mut self, ctx: &Context) {
        let dirty = self.workspace.group_ids().into_iter().any(|group| {
            self.workspace.group_by_id(group).is_some_and(|g| {
                g.tabs()
                    .iter()
                    .any(|&id| self.workspace.document(id).is_some_and(Document::is_dirty))
            })
        });
        if dirty && !self.confirm_quit {
            self.confirm_quit = true;
            self.message = Some(QUIT_PROMPT.to_owned());
        } else {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// Opens the quick input with `prefix` typed; Ctrl+P in it selects the
    /// next row instead, and the other keys start it again with theirs.
    pub(super) fn open_quick(&mut self, ctx: &Context, prefix: &str) {
        match self.quick.as_mut() {
            Some(quick) if prefix.is_empty() => quick.select_next(),
            Some(quick) => quick.restart(ctx, prefix),
            None => self.quick = Some(QuickInput::new(ctx, prefix)),
        }
    }

    /// Runs what closed the quick input.
    fn close_quick(&mut self, ctx: &Context, close: Close) {
        let previous = self.quick.take().and_then(|quick| quick.previous());
        match close {
            Close::Accept(Accepted::File(path)) => match self.workspace.open(&path) {
                Ok(_) => self.focus_editor(ctx),
                Err(err) => self.message = Some(err.to_string()),
            },
            Close::Accept(Accepted::Command(id)) => {
                self.refocus(ctx, previous);
                self.run_command(ctx, id);
            }
            Close::Accept(Accepted::Line { line, column }) => {
                if let Some(id) = self.workspace.active_document() {
                    if let Err(err) = self.workspace.go_to_line(id, line, column) {
                        self.message = Some(err.to_string());
                    }
                    self.reveal();
                }
                self.focus_editor(ctx);
            }
            Close::Cancel => self.refocus(ctx, previous),
            Close::Blur => {}
        }
    }

    /// Gives focus back to `previous`, or to the editor or the explorer.
    fn refocus(&self, ctx: &Context, previous: Option<egui::Id>) {
        match previous {
            Some(id) => focus(ctx, id),
            None if self.workspace.active_document().is_some() => focus(ctx, editor::id()),
            None if self.sidebar.visible() && self.workspace.explorer().is_some() => {
                focus(ctx, explorer::id());
            }
            None => {}
        }
    }

    /// Runs a command of the registry chosen in the palette: a workbench
    /// command here, any other in the workspace, on the active document.
    pub(super) fn run_command(&mut self, ctx: &Context, id: &str) {
        if let Some(command) = Workbench::from_id(id) {
            self.run_workbench(ctx, command);
            return;
        }
        if groups::is_group_command(id) {
            self.run_group_command(ctx, id);
            return;
        }
        if let Some(outcome) = self.navigator.run(&mut self.workspace, id) {
            self.navigate_outcome(ctx, outcome);
            return;
        }
        match self.workspace.execute(id) {
            Ok(true) => self.reveal(),
            Ok(false) => self.message = Some(format!("{id} is not available in the GUI")),
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    /// The quick input and the find widget, above the workbench; they run
    /// before it, so the editor draws what they did this frame.
    pub(super) fn show_overlays(&mut self, ui: &mut egui::Ui, areas: &Areas) -> bool {
        let ctx = ui.ctx().clone();
        let mut changed = false;
        if ctx.memory(|memory| memory.has_focus(editor::id())) {
            let outcome = find::editor_keys(&ctx, &mut self.workspace, &mut self.find);
            changed |= self.find_outcome(&ctx, outcome);
        }
        let outcome = find::show(ui, &mut self.workspace, &mut self.find, areas.editor);
        changed |= self.find_outcome(&ctx, outcome);
        if ctx.memory(|memory| memory.has_focus(editor::id()))
            && let Some(id) = navigate::keys(&ctx)
        {
            self.run_command(&ctx, id);
        }
        let outcome = self.navigator.show(&ctx, &mut self.workspace);
        changed |= !matches!(outcome, navigate::Outcome::None);
        self.navigate_outcome(&ctx, outcome);
        if let Some(quick) = self.quick.as_mut() {
            let place = quick_input::Place {
                group: areas.tabs.union(areas.editor),
                window: ui.max_rect(),
            };
            if let Some(close) = quick_input::show(ui, &mut self.workspace, quick, place) {
                self.close_quick(&ctx, close);
                changed = true;
            }
        }
        changed
    }

    /// What the location list or the rename box did.
    fn navigate_outcome(&mut self, ctx: &Context, outcome: navigate::Outcome) {
        match outcome {
            navigate::Outcome::None => {}
            navigate::Outcome::Closed => self.focus_editor(ctx),
            navigate::Outcome::Picked(id) => {
                self.view_of(id).reveal();
                self.focus_editor_next = true;
                ctx.request_repaint();
            }
            navigate::Outcome::Message(message) => self.message = Some(message),
        }
    }

    fn find_outcome(&mut self, ctx: &Context, outcome: find::Outcome) -> bool {
        if outcome.reveal {
            self.reveal();
        }
        if let Some(message) = outcome.message {
            self.message = Some(message);
        }
        if outcome.focus_editor {
            self.focus_editor(ctx);
        }
        outcome.changed
    }
}
