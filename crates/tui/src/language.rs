//! "Change Language Mode" in the terminal: the quick input box listing
//! the languages, opened by the palette command
//! (`workbench.action.editor.changeLanguageMode`) or a click on the
//! status bar's language name, as VS Code's status bar item does
//! (`src/vs/workbench/browser/parts/editor/editorStatus.ts:577-589`). The
//! list and the filtering are the workspace's
//! ([`Workspace::language_picks`](rustode_workspace::Workspace::language_picks)).

use crate::app::{App, Focus, QuickInput, QuickItem};
use rustode_workspace::LanguagePick;

impl App {
    /// Opens the language picker for the active document, or says there
    /// is none (VS Code shows "No text editor active at this time").
    pub(crate) fn open_language_picker(&mut self) {
        let Some(id) = self.document() else {
            self.message = Some("No text editor active at this time".to_owned());
            return;
        };
        self.quick = Some(QuickInput {
            text: String::new(),
            selected: 0,
            language: Some(id),
            ..QuickInput::default()
        });
    }

    /// The picker's rows for `query`, when the quick input is the picker.
    pub(crate) fn language_items(&self, query: &str) -> Option<Vec<QuickItem>> {
        let id = self.quick.as_ref()?.language?;
        let picks = self.workspace.language_picks(id, query).unwrap_or_default();
        Some(picks.into_iter().map(QuickItem::Language).collect())
    }

    /// Applies a pick to the document the picker was opened for.
    pub(crate) fn pick_language(&mut self, pick: &LanguagePick) {
        let Some(id) = self.document() else { return };
        match self.workspace.set_language(id, pick.id) {
            Ok(()) => self.focus = Focus::Editor,
            Err(err) => self.message = Some(err.to_string()),
        }
    }
}
