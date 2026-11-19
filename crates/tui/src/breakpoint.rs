//! The breakpoint editor in the terminal: "Add Conditional Breakpoint...",
//! "Add Logpoint..." and "Edit Breakpoint" open the quick input box on the
//! cursor's line, as VS Code opens its breakpoint widget under the line
//! (`src/vs/workbench/contrib/debug/browser/breakpointWidget.ts:92-94`,
//! `:215-217`; `debugEditorActions.ts:101-215`). What the box holds, and what
//! applying it does, is the workspace's (`begin_breakpoint_edit`,
//! `finish_breakpoint_edit`); this module only types into it. The box shows
//! one of the three texts (Expression, Hit Count, Log Message); Tab shows
//! the next one, the others keep what was typed; Enter applies, Escape
//! cancels.

use crate::app::{App, QuickInput, QuickItem};
use rustode_workspace::BreakpointInput;

impl App {
    /// Runs one of the breakpoint editor commands by id; returns whether
    /// `id` is one.
    pub(crate) fn run_breakpoint_command(&mut self, id: &str) -> bool {
        let input = match id {
            "editor.debug.action.conditionalBreakpoint" => Some(BreakpointInput::Expression),
            "editor.debug.action.addLogPoint" => Some(BreakpointInput::LogMessage),
            "editor.debug.action.editBreakpoint" => None,
            _ => return false,
        };
        let Some(document) = self.document() else {
            self.message = Some("No text editor active at this time".to_owned());
            return true;
        };
        let line = self
            .workspace
            .document(document)
            .map_or(0, |d| d.cursor_position().line);
        match self.workspace.begin_breakpoint_edit(document, line, input) {
            Ok(draft) => {
                self.quick = Some(QuickInput {
                    text: draft.text().to_owned(),
                    breakpoint: Some(draft),
                    ..QuickInput::default()
                });
            }
            Err(err) => self.message = Some(err.to_string()),
        }
        true
    }

    /// Tab: the box shows the next of the three texts.
    pub(crate) fn next_breakpoint_input(&mut self) {
        if let Some(quick) = self.quick.as_mut()
            && let Some(draft) = quick.breakpoint.as_mut()
        {
            draft.set_text(&quick.text);
            draft.next_input();
            quick.text = draft.text().to_owned();
        }
    }

    /// Enter: applies what was typed.
    pub(crate) fn finish_breakpoint(&mut self, quick: QuickInput) {
        let Some(mut draft) = quick.breakpoint else {
            return;
        };
        draft.set_text(&quick.text);
        match self.workspace.finish_breakpoint_edit(&draft) {
            Ok(()) => self.message = None,
            Err(err) => self.message = Some(err.to_string()),
        }
    }

    /// The row under the box while it edits a breakpoint: which text it is
    /// and what the keys do.
    pub(crate) fn breakpoint_items(&self) -> Option<Vec<QuickItem>> {
        let draft = self.quick.as_ref()?.breakpoint.as_ref()?;
        Some(vec![
            QuickItem::Hint(format!("{}: {}", draft.input.label(), draft.input.hint())),
            QuickItem::Hint("Tab: next, Enter: accept, Escape: cancel".to_owned()),
        ])
    }
}
