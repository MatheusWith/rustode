//! The disk-conflict prompt: saving a document whose file changed under its
//! unsaved edits asks what to do instead of choosing for the user. The
//! workspace decides the sentence and the choices
//! ([`Workspace::conflict_prompt`](rustode_workspace::Workspace::conflict_prompt));
//! each choice is a registered command, so the palette and a user's
//! `keybindings.json` reach them too. The first letter of a choice's label
//! is its key.

use crate::app::App;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rustode_workspace::ConflictPrompt;

impl App {
    /// Shows the prompt for the active document, if its save hit a conflict.
    pub(crate) fn open_conflict_prompt(&mut self) {
        let Some(id) = self.document() else { return };
        self.conflict = self.workspace.conflict_prompt(id);
        self.message = self
            .conflict
            .as_ref()
            .map(|prompt| self.prompt_line(prompt));
    }

    /// Closes the prompt once its conflict is gone, however that happened
    /// (a choice, the palette, a reload).
    pub(crate) fn refresh_conflict_prompt(&mut self) {
        let Some(prompt) = &self.conflict else { return };
        if self
            .workspace
            .document(prompt.document)
            .is_none_or(|d| !d.has_conflict())
        {
            self.conflict = None;
            self.message = None;
        }
    }

    /// Puts the prompt back on the status bar after a later message (a
    /// queued disk event) replaced it.
    pub(crate) fn restore_conflict_prompt(&mut self) {
        if let Some(prompt) = self.conflict.clone() {
            self.message = Some(self.prompt_line(&prompt));
        }
    }

    /// One row: what happened, then each choice with its key.
    fn prompt_line(&self, prompt: &ConflictPrompt) -> String {
        let name = self
            .workspace
            .document(prompt.document)
            .map(rustode_workspace::Document::name)
            .unwrap_or_default();
        let mut line = format!("{name} changed on disk:");
        for choice in &prompt.choices {
            let mut letters = choice.label.chars();
            if let Some(first) = letters.next() {
                line.extend([" [", &first.to_string(), "]", letters.as_str()]);
            }
        }
        line.push_str(" [Esc]");
        line
    }

    /// A key while the prompt is open: a choice's first letter runs its
    /// command, Esc closes the prompt, anything else is ignored.
    pub(crate) fn conflict_key(&mut self, key: KeyEvent) {
        let Some(prompt) = self.conflict.clone() else {
            return;
        };
        if key.code == KeyCode::Esc {
            self.conflict = None;
            self.message = None;
            return;
        }
        let KeyCode::Char(typed) = key.code else {
            return;
        };
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return;
        }
        let choice = prompt.choices.iter().find(|c| {
            c.label
                .chars()
                .next()
                .is_some_and(|f| f.eq_ignore_ascii_case(&typed))
        });
        let Some(choice) = choice else { return };
        self.conflict = None;
        self.message = None;
        self.run_id(choice.command);
        if self.message.is_none() {
            self.message = Some(
                if choice.command == "workbench.files.action.saveIgnoreModifiedSince" {
                    "Saved".to_owned()
                } else {
                    "Reverted to the file's contents".to_owned()
                },
            );
        }
    }
}
