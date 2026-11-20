//! Editor groups in the terminal: the keys that split, focus and move
//! between them. The workspace owns the layout (`Workspace::layout`) and
//! runs every command; this module only maps keys to VS Code's ids.
//!
//! Terminals without an extended keyboard protocol send Ctrl+\ as Ctrl+4
//! (byte 0x1C), which is accepted too. Ctrl+1 to Ctrl+3 need the extended
//! protocol; the command palette runs every group command in any terminal.
//! Ctrl+K then Ctrl+arrow focuses the group in that direction, and Ctrl+K W
//! closes the group's editors, as in VS Code's default keybindings
//! (`src/vs/workbench/browser/parts/editor/editorActions.ts`, `editorCommands.ts`).

use crate::app::{App, Focus};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The group command a single chord runs.
pub(crate) fn key_command(key: KeyEvent) -> Option<&'static str> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    if !ctrl || shift {
        return None;
    }
    match (key.code, alt) {
        (KeyCode::Char('\\' | '4'), false) => Some("workbench.action.splitEditor"),
        (KeyCode::Char('1'), false) => Some("workbench.action.focusFirstEditorGroup"),
        (KeyCode::Char('2'), false) => Some("workbench.action.focusSecondEditorGroup"),
        (KeyCode::Char('3'), false) => Some("workbench.action.focusThirdEditorGroup"),
        (KeyCode::Right, true) => Some("workbench.action.moveEditorToNextGroup"),
        (KeyCode::Left, true) => Some("workbench.action.moveEditorToPreviousGroup"),
        _ => None,
    }
}

/// The group command that completes Ctrl+K.
pub(crate) fn chord_command(key: KeyEvent) -> Option<&'static str> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Left if ctrl => Some("workbench.action.focusLeftGroup"),
        KeyCode::Right if ctrl => Some("workbench.action.focusRightGroup"),
        KeyCode::Up if ctrl => Some("workbench.action.focusAboveGroup"),
        KeyCode::Down if ctrl => Some("workbench.action.focusBelowGroup"),
        KeyCode::Char('w' | 'W') => Some("workbench.action.closeEditorsInGroup"),
        _ => None,
    }
}

impl App {
    /// Runs the group command `key` stands for; returns whether it was one.
    pub(crate) fn group_key(&mut self, key: KeyEvent) -> bool {
        let Some(id) = key_command(key) else {
            return false;
        };
        self.run_group_command(id);
        true
    }

    /// Runs a group command by id, then moves the focus into the editor.
    pub(crate) fn run_group_command(&mut self, id: &str) {
        self.message = None;
        self.confirm_close = None;
        match self.workspace.execute(id) {
            Ok(_) => self.focus = Focus::Editor,
            Err(err) => self.message = Some(err.to_string()),
        }
    }
}
