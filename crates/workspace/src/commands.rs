//! The command registry: every command a frontend can run by name, with VS
//! Code's ids, palette labels and default Linux keybindings, plus the user's
//! `keybindings.json` overrides.
//!
//! Ids, labels and keys follow VS Code, e.g. `workbench.action.files.save`
//! "Save" (`src/vs/workbench/contrib/files/browser/fileConstants.ts:23-24`),
//! "Go to File..." on Ctrl+P (`src/vs/workbench/browser/actions/quickAccessActions.ts:128-129`),
//! "Show All Commands" on Ctrl+Shift+P or F1
//! (`src/vs/workbench/contrib/quickaccess/browser/commandsQuickAccess.ts:275-285`),
//! "Go to Line/Column..." on Ctrl+G
//! (`src/vs/workbench/contrib/codeEditor/browser/quickaccess/gotoLineQuickAccess.ts:71-84`),
//! redo on Ctrl+Y (`src/vs/editor/browser/editorExtensions.ts:673-680`),
//! "View: Show Extensions" on Ctrl+Shift+X
//! (`src/vs/workbench/contrib/extensions/browser/extensions.contribution.ts:116-123`,
//! titled "Show {0}" in the View category by
//! `src/vs/workbench/services/views/browser/viewsService.ts:404-421`).
//! Commands that take an argument ([`Workspace::execute_with`]) are not
//! listed in the palette, as in VS Code.
//! "View: Toggle Problems" on Ctrl+Shift+M
//! (`src/vs/workbench/contrib/markers/browser/markers.contribution.ts:150-154`,
//! titled by `src/vs/workbench/services/views/browser/viewsService.ts:481-489`).
//! "Language Server: Restart" (`rustode.restartLanguageServer`) is rustode's
//! own: VS Code's language clients bring their own restart commands.
//! The palette lists matching commands alphabetically; VS Code also puts
//! recently used ones first and matches word starts
//! (`AbstractCommandsQuickAccessProvider`), which is not done yet.

use crate::{DocumentId, Error, Workspace};
use rustode_config::Chord;

/// Who runs a command: the workspace on the active document, or the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandScope {
    /// [`Workspace::execute`] runs it on the active document.
    Editor,
    /// The frontend runs it (focus, quit, quick input, prompts).
    Workbench,
}

/// A command a frontend can show and run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandInfo {
    /// VS Code's command id.
    pub id: &'static str,
    /// The palette label, with its category, e.g. `File: Save`.
    pub label: &'static str,
    /// Who runs it.
    pub scope: CommandScope,
    default_key: Option<&'static str>,
}

const fn command(
    id: &'static str,
    label: &'static str,
    scope: CommandScope,
    default_key: Option<&'static str>,
) -> CommandInfo {
    CommandInfo {
        id,
        label,
        scope,
        default_key,
    }
}

use CommandScope::{Editor, Workbench};

/// Every registered command.
pub const COMMANDS: &[CommandInfo] = &[
    command(
        "workbench.action.files.save",
        "File: Save",
        Workbench,
        Some("Ctrl+S"),
    ),
    command(
        "workbench.action.quit",
        "File: Quit",
        Workbench,
        Some("Ctrl+Q"),
    ),
    command(
        "workbench.action.closeActiveEditor",
        "View: Close Editor",
        Workbench,
        Some("Ctrl+W"),
    ),
    command(
        "workbench.action.nextEditor",
        "View: Open Next Editor",
        Editor,
        Some("Ctrl+PageDown"),
    ),
    command(
        "workbench.action.previousEditor",
        "View: Open Previous Editor",
        Editor,
        Some("Ctrl+PageUp"),
    ),
    command(
        "workbench.view.explorer",
        "View: Show Explorer",
        Workbench,
        Some("Ctrl+Shift+E"),
    ),
    command(
        "workbench.view.extensions",
        "View: Show Extensions",
        Workbench,
        Some("Ctrl+Shift+X"),
    ),
    command(
        "workbench.action.focusNextPart",
        "View: Focus Next Part",
        Workbench,
        Some("F6"),
    ),
    command(
        "workbench.action.quickOpen",
        "Go to File...",
        Workbench,
        Some("Ctrl+P"),
    ),
    command(
        "workbench.action.showCommands",
        "Show All Commands",
        Workbench,
        Some("Ctrl+Shift+P"),
    ),
    command(
        "workbench.action.gotoLine",
        "Go to Line/Column...",
        Workbench,
        Some("Ctrl+G"),
    ),
    command("undo", "Undo", Editor, Some("Ctrl+Z")),
    command("redo", "Redo", Editor, Some("Ctrl+Y")),
    command(
        "editor.action.selectAll",
        "Select All",
        Editor,
        Some("Ctrl+A"),
    ),
    command(
        "editor.action.addSelectionToNextFindMatch",
        "Add Selection To Next Find Match",
        Editor,
        Some("Ctrl+D"),
    ),
    command(
        "editor.action.insertCursorAbove",
        "Add Cursor Above",
        Editor,
        Some("Shift+Alt+UpArrow"),
    ),
    command(
        "editor.action.insertCursorBelow",
        "Add Cursor Below",
        Editor,
        Some("Shift+Alt+DownArrow"),
    ),
    command(
        "cursorTop",
        "Go to Beginning of File",
        Editor,
        Some("Ctrl+Home"),
    ),
    command(
        "cursorBottom",
        "Go to End of File",
        Editor,
        Some("Ctrl+End"),
    ),
    command(
        "workbench.actions.view.problems",
        "View: Toggle Problems",
        Workbench,
        Some("Ctrl+Shift+M"),
    ),
    command(
        "rustode.restartLanguageServer",
        "Language Server: Restart",
        Editor,
        None,
    ),
];

/// A command that matched a palette query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandMatch {
    /// The command.
    pub command: &'static CommandInfo,
    /// Its keybinding as shown, after the user's overrides.
    pub keybinding: Option<String>,
    /// Matched chars of the label.
    pub positions: Vec<usize>,
}

/// `query`'s space-separated words found in `label`, case-insensitively.
fn match_label(label: &str, query: &str) -> Option<Vec<usize>> {
    let lower: Vec<char> = label.chars().flat_map(char::to_lowercase).collect();
    let mut positions = Vec::new();
    for word in query.split_whitespace() {
        let word: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
        let start = lower
            .windows(word.len())
            .position(|w| w == word.as_slice())?;
        positions.extend(start..start + word.len());
    }
    positions.sort_unstable();
    positions.dedup();
    Some(positions)
}

/// A key chord as VS Code labels it on Linux, e.g. `Ctrl+Shift+P`.
fn chord_label(chord: &Chord) -> String {
    let mut parts = Vec::new();
    for (on, name) in [
        (chord.ctrl, "Ctrl"),
        (chord.shift, "Shift"),
        (chord.alt, "Alt"),
        (chord.meta, "Super"),
    ] {
        if on {
            parts.push(name.to_owned());
        }
    }
    let key = match chord.key.as_str() {
        "up" => "UpArrow".to_owned(),
        "down" => "DownArrow".to_owned(),
        "left" => "LeftArrow".to_owned(),
        "right" => "RightArrow".to_owned(),
        "pageup" => "PageUp".to_owned(),
        "pagedown" => "PageDown".to_owned(),
        "escape" => "Escape".to_owned(),
        other => {
            let mut chars = other.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        }
    };
    parts.push(key);
    parts.join("+")
}

impl Workspace {
    /// The commands matching `query` (all for an empty query), sorted by label.
    #[must_use]
    pub fn commands(&self, query: &str) -> Vec<CommandMatch> {
        let mut matches: Vec<CommandMatch> = COMMANDS
            .iter()
            .filter_map(|command| {
                let positions = match_label(command.label, query)?;
                Some(CommandMatch {
                    command,
                    keybinding: self.keybinding_label(command.id),
                    positions,
                })
            })
            .collect();
        matches.sort_by(|a, b| a.command.label.cmp(b.command.label));
        matches
    }

    /// A command's keybinding as shown: the last one `keybindings.json` adds,
    /// else the default unless the file removes it (`"-command"`).
    #[must_use]
    pub fn keybinding_label(&self, id: &str) -> Option<String> {
        let mut label = COMMANDS
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.default_key.map(str::to_owned));
        for binding in self.config.keybindings() {
            let Some(command) = binding.command.as_deref() else {
                continue;
            };
            if command.strip_prefix('-') == Some(id) {
                label = None;
            } else if command == id
                && let Some(chords) = &binding.key
            {
                let chords: Vec<String> = chords.iter().map(chord_label).collect();
                label = Some(chords.join(" "));
            }
        }
        label
    }

    /// Runs an editor command on the active document. Returns `false` for
    /// workbench commands, which the frontend runs, and when no document is
    /// active.
    ///
    /// # Errors
    /// When the command fails, e.g. an edit is refused.
    pub fn execute(&mut self, id: &str) -> Result<bool, Error> {
        match id {
            "workbench.action.nextEditor" => self.next_tab(),
            "workbench.action.previousEditor" => self.previous_tab(),
            _ => {
                let Some(document) = self.active_document() else {
                    return Ok(false);
                };
                return self.execute_on(document, id);
            }
        }
        Ok(true)
    }

    /// Runs a command that takes one argument, by VS Code id:
    /// `workbench.extensions.installExtension` and
    /// `workbench.extensions.uninstallExtension` take an extension id,
    /// `workbench.extensions.search` a search text
    /// (`src/vs/workbench/contrib/extensions/browser/extensions.contribution.ts:432-580`).
    /// Returns `false` for an unknown command.
    ///
    /// # Errors
    /// When an extension command is refused (see
    /// [`Workspace::install_extension`]).
    pub fn execute_with(&mut self, id: &str, arg: &str) -> Result<bool, crate::ExtensionError> {
        match id {
            "workbench.extensions.installExtension" => self.install_extension(arg)?,
            "workbench.extensions.uninstallExtension" => self.uninstall_extension(arg)?,
            "workbench.extensions.search" => self.search_extensions(arg),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn execute_on(&mut self, id: DocumentId, command: &str) -> Result<bool, Error> {
        use rustode_core::Movement;
        match command {
            "undo" => drop(self.undo(id)?),
            "redo" => drop(self.redo(id)?),
            "editor.action.selectAll" => self.select_all(id)?,
            "editor.action.addSelectionToNextFindMatch" => drop(self.add_next_occurrence(id)?),
            "editor.action.insertCursorAbove" => drop(self.add_cursor_above(id)?),
            "editor.action.insertCursorBelow" => drop(self.add_cursor_below(id)?),
            "cursorTop" => self.move_cursor(id, Movement::DocumentStart, false)?,
            "cursorBottom" => self.move_cursor(id, Movement::DocumentEnd, false)?,
            "rustode.restartLanguageServer" => return self.restart_language_server(id),
            _ => return Ok(false),
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_a_unique_id() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    #[test]
    fn chords_are_labelled_like_vs_code() {
        let chord = Chord {
            ctrl: true,
            shift: true,
            alt: false,
            meta: false,
            key: "pagedown".to_owned(),
        };
        assert_eq!(chord_label(&chord), "Ctrl+Shift+PageDown");
        assert_eq!(match_label("File: Save", "fi sa"), Some(vec![0, 1, 6, 7]));
        assert_eq!(match_label("File: Save", "quit"), None);
    }
}
