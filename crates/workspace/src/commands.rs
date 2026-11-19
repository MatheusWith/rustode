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
//! `src/vs/workbench/services/views/browser/viewsService.ts:404-421`),
//! "Search: Find in Files" on Ctrl+Shift+F
//! (`src/vs/workbench/contrib/search/browser/searchActionsFind.ts:203-242`,
//! in the Search category of `searchActionsBase.ts:25`).
//! "Search: Replace in Files" on Ctrl+Shift+H
//! (`src/vs/workbench/contrib/search/browser/searchActionsNav.ts:396-413`),
//! "Search: Replace" and "Search: Replace All in File" on Ctrl+Shift+1,
//! each for its own kind of result (a match, a file), and "Search: Dismiss"
//! on Delete (`src/vs/workbench/contrib/search/browser/searchActionsRemoveReplace.ts:59-68`,
//! `:154-160`, `:190-196`); "Search: Replace All" is the Search view's
//! button (`searchView.ts:814-856`) and "Search: Undo Replace"
//! (`rustode.search.undoReplace`) is rustode's own: VS Code undoes a bulk
//! edit from the editor, rustode has no tab-less models. The frontend runs
//! them on the selected result row.
//! Commands that take an argument ([`Workspace::execute_with`]) are not
//! listed in the palette, as in VS Code.
//! "View: Toggle Problems" on Ctrl+Shift+M
//! (`src/vs/workbench/contrib/markers/browser/markers.contribution.ts:150-154`,
//! titled by `src/vs/workbench/services/views/browser/viewsService.ts:481-489`).
//! "Trigger Suggest" on Ctrl+Space
//! (`src/vs/editor/contrib/suggest/browser/suggestController.ts:797-812`) and
//! "Show or Focus Hover" on Ctrl+K Ctrl+I
//! (`src/vs/editor/contrib/hover/browser/hoverActions.ts:28-70`); both return
//! `false` when the document's server cannot answer.
//! "Go to Definition" on F12
//! (`src/vs/editor/contrib/gotoSymbol/browser/goToCommands.ts:276-296`),
//! "Go to References" on Shift+F12 (`goToCommands.ts:669-681`), "Rename
//! Symbol" on F2 (`src/vs/editor/contrib/rename/browser/rename.ts:365-370`;
//! it needs the new name, so the frontend asks for it and calls
//! [`Workspace::rename_symbol`]) and "Format Document" on Ctrl+Shift+I, the
//! Linux key (`src/vs/editor/contrib/format/browser/formatActions.ts:218-224`);
//! like the two above, they return `false` when the document's server cannot
//! answer.
//! "Change Language Mode" on Ctrl+K M
//! (`src/vs/workbench/browser/parts/editor/editorStatus.ts:1110-1131`; the
//! frontends open the picker, [`Workspace::language_picks`]).
//! "Debug: Start Debugging" on F5, "Debug: Continue" on F5 while stopped,
//! "Debug: Stop" on Shift+F5 and "Debug: Toggle Breakpoint" on F9
//! (`src/vs/workbench/contrib/debug/browser/debugCommands.ts:48-72`, `:650`,
//! `:687`, `:767`, `:1169`; `debugEditorActions.ts:35-60`); the first three run
//! without a document, and [`Workspace::debug_key_command`] says which of
//! start and continue F5 means now.
//! "Debug: Add Conditional Breakpoint...", "Add Logpoint..." and "Edit
//! Breakpoint" have no key in VS Code
//! (`src/vs/workbench/contrib/debug/browser/debugEditorActions.ts:101-215`:
//! `editor.debug.action.conditionalBreakpoint`, `.addLogPoint`,
//! `.editBreakpoint`); they open the breakpoint editor, so the frontend asks
//! for the texts ([`Workspace::begin_breakpoint_edit`]) and calls
//! [`Workspace::finish_breakpoint_edit`]. `debug.editBreakpoint` is VS Code's
//! id for editing a *function* breakpoint's condition
//! (`breakpointsView.ts:2353`), which rustode does not have.
//! "Debug: Step Over" on F10, "Step Into" on F11, "Step Out" on Shift+F11
//! and "Pause" on F6, each with VS Code's `when` clause
//! (`debugCommands.ts:500-560`: stopped; not inactive; stopped with more than
//! one frame; running); [`Workspace::command_applies`] evaluates it, which is
//! how F6 pauses while the program runs and moves the focus otherwise.
//! "Debug: Add Watch Expression" (`watchExpressionsView.ts:612`), "Remove
//! Watch Expression" (`debugCommands.ts:77`; Delete in the Watch view) and the
//! four "Focus on … View" commands (`debug.contribution.ts:471-473`,
//! `repl.ts:1228`) are workbench commands: the frontend asks for the text
//! or moves the focus.
//! The editor group commands (`groups.rs`): "View: Split Editor" on Ctrl+\
//! (`src/vs/workbench/browser/parts/editor/editorActions.ts:82-95`), the
//! "Focus … Editor Group" family on Ctrl+1 to Ctrl+3 (`editorCommands.ts:694-740`)
//! and Ctrl+K then Ctrl+arrow (`editorActions.ts:347-420`), "Move Editor into
//! Next/Previous Group" on Ctrl+Alt+Right and Left (`editorActions.ts:2128-2156`),
//! "Close All Editors in Group" on Ctrl+K W and "Close Editor Group" on
//! Ctrl+W while the group is empty (`editorCommands.ts:853-880`), which is
//! why "Close Editor" carries `!activeEditorGroupEmpty`.
//! "Language Server: Restart" (`rustode.restartLanguageServer`) is rustode's
//! own: VS Code's language clients bring their own restart commands.
//! The palette lists matching commands alphabetically; VS Code also puts
//! recently used ones first and matches word starts
//! (`AbstractCommandsQuickAccessProvider`), which is not done yet.

use crate::{Direction, DocumentId, Error, SplitDirection, StepKind, Workspace};
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
    /// The `when` clause of the default key, in VS Code's syntax: the key
    /// means this command only while it holds
    /// ([`Workspace::command_applies`]).
    pub(crate) when: Option<&'static str>,
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
        when: None,
    }
}

impl CommandInfo {
    /// The same command, its default key limited by a `when` clause.
    const fn when(mut self, clause: &'static str) -> Self {
        self.when = Some(clause);
        self
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
    )
    .when("!activeEditorGroupEmpty"),
    command(
        "workbench.action.closeGroup",
        "View: Close Editor Group",
        Editor,
        Some("Ctrl+W"),
    )
    .when("activeEditorGroupEmpty && multipleEditorGroups"),
    command(
        "workbench.action.closeEditorsInGroup",
        "View: Close All Editors in Group",
        Editor,
        Some("Ctrl+K W"),
    ),
    command(
        "workbench.action.splitEditor",
        "View: Split Editor",
        Editor,
        Some("Ctrl+\\"),
    ),
    command(
        "workbench.action.splitEditorRight",
        "View: Split Editor Right",
        Editor,
        None,
    ),
    command(
        "workbench.action.splitEditorDown",
        "View: Split Editor Down",
        Editor,
        None,
    ),
    command(
        "workbench.action.focusFirstEditorGroup",
        "View: Focus First Editor Group",
        Editor,
        Some("Ctrl+1"),
    ),
    command(
        "workbench.action.focusSecondEditorGroup",
        "View: Focus Second Editor Group",
        Editor,
        Some("Ctrl+2"),
    ),
    command(
        "workbench.action.focusThirdEditorGroup",
        "View: Focus Third Editor Group",
        Editor,
        Some("Ctrl+3"),
    ),
    command(
        "workbench.action.focusNextGroup",
        "View: Focus Next Editor Group",
        Editor,
        None,
    ),
    command(
        "workbench.action.focusPreviousGroup",
        "View: Focus Previous Editor Group",
        Editor,
        None,
    ),
    command(
        "workbench.action.focusLeftGroup",
        "View: Focus Left Editor Group",
        Editor,
        Some("Ctrl+K Ctrl+Left"),
    ),
    command(
        "workbench.action.focusRightGroup",
        "View: Focus Right Editor Group",
        Editor,
        Some("Ctrl+K Ctrl+Right"),
    ),
    command(
        "workbench.action.focusAboveGroup",
        "View: Focus Editor Group Above",
        Editor,
        Some("Ctrl+K Ctrl+Up"),
    ),
    command(
        "workbench.action.focusBelowGroup",
        "View: Focus Editor Group Below",
        Editor,
        Some("Ctrl+K Ctrl+Down"),
    ),
    command(
        "workbench.action.moveEditorToNextGroup",
        "View: Move Editor into Next Group",
        Editor,
        Some("Ctrl+Alt+Right"),
    ),
    command(
        "workbench.action.moveEditorToPreviousGroup",
        "View: Move Editor into Previous Group",
        Editor,
        Some("Ctrl+Alt+Left"),
    ),
    command(
        "workbench.action.joinAllGroups",
        "View: Join All Editor Groups",
        Editor,
        None,
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
        "workbench.action.findInFiles",
        "Search: Find in Files",
        Workbench,
        Some("Ctrl+Shift+F"),
    ),
    command(
        "workbench.action.replaceInFiles",
        "Search: Replace in Files",
        Workbench,
        Some("Ctrl+Shift+H"),
    ),
    command(
        "search.action.replaceAll",
        "Search: Replace All",
        Workbench,
        None,
    ),
    command(
        "search.action.replace",
        "Search: Replace",
        Workbench,
        Some("Ctrl+Shift+1"),
    ),
    command(
        "search.action.replaceAllInFile",
        "Search: Replace All in File",
        Workbench,
        Some("Ctrl+Shift+1"),
    ),
    command(
        "search.action.remove",
        "Search: Dismiss",
        Workbench,
        Some("Delete"),
    ),
    command(
        "rustode.search.undoReplace",
        "Search: Undo Replace",
        Workbench,
        None,
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
        "editor.action.toggleMinimap",
        "View: Toggle Minimap",
        Workbench,
        None,
    ),
    command(
        "editor.action.triggerSuggest",
        "Trigger Suggest",
        Editor,
        Some("Ctrl+Space"),
    ),
    command(
        "editor.action.showHover",
        "Show or Focus Hover",
        Editor,
        Some("Ctrl+K Ctrl+I"),
    ),
    command(
        "editor.action.revealDefinition",
        "Go to Definition",
        Editor,
        Some("F12"),
    ),
    command(
        "editor.action.goToReferences",
        "Go to References",
        Editor,
        Some("Shift+F12"),
    ),
    command(
        "editor.action.rename",
        "Rename Symbol",
        Workbench,
        Some("F2"),
    ),
    command(
        "editor.action.formatDocument",
        "Format Document",
        Editor,
        Some("Ctrl+Shift+I"),
    ),
    command(
        "workbench.action.editor.changeLanguageMode",
        "Change Language Mode",
        Workbench,
        Some("Ctrl+K M"),
    ),
    command(
        "workbench.action.debug.start",
        "Debug: Start Debugging",
        Editor,
        Some("F5"),
    ),
    command(
        "workbench.action.debug.continue",
        "Debug: Continue",
        Editor,
        Some("F5"),
    ),
    command(
        "workbench.action.debug.stop",
        "Debug: Stop",
        Editor,
        Some("Shift+F5"),
    ),
    command(
        "editor.debug.action.toggleBreakpoint",
        "Debug: Toggle Breakpoint",
        Editor,
        Some("F9"),
    ),
    command(
        "editor.debug.action.conditionalBreakpoint",
        "Debug: Add Conditional Breakpoint...",
        Editor,
        None,
    ),
    command(
        "editor.debug.action.addLogPoint",
        "Debug: Add Logpoint...",
        Editor,
        None,
    ),
    command(
        "editor.debug.action.editBreakpoint",
        "Debug: Edit Breakpoint",
        Editor,
        None,
    ),
    command(
        "workbench.action.debug.stepOver",
        "Debug: Step Over",
        Editor,
        Some("F10"),
    )
    .when("debugState == 'stopped'"),
    command(
        "workbench.action.debug.stepInto",
        "Debug: Step Into",
        Editor,
        Some("F11"),
    )
    .when("debugState != 'inactive'"),
    command(
        "workbench.action.debug.stepOut",
        "Debug: Step Out",
        Editor,
        Some("Shift+F11"),
    )
    .when("debugState == 'stopped' && threadHasMultipleStackFrames"),
    command(
        "workbench.action.debug.pause",
        "Debug: Pause",
        Editor,
        Some("F6"),
    )
    .when("debugState == 'running'"),
    command(
        "workbench.debug.viewlet.action.addWatchExpression",
        "Debug: Add Watch Expression",
        Workbench,
        None,
    ),
    command(
        "debug.removeWatchExpression",
        "Debug: Remove Watch Expression",
        Workbench,
        Some("Delete"),
    )
    .when("watchExpressionsFocused"),
    command(
        "workbench.debug.viewlet.action.removeAllWatchExpressions",
        "Debug: Remove All Watch Expressions",
        Editor,
        None,
    ),
    command(
        "workbench.debug.action.focusCallStackView",
        "Debug: Focus on Call Stack View",
        Workbench,
        None,
    ),
    command(
        "workbench.debug.action.focusVariablesView",
        "Debug: Focus on Variables View",
        Workbench,
        None,
    ),
    command(
        "workbench.debug.action.focusWatchView",
        "Debug: Focus on Watch View",
        Workbench,
        None,
    ),
    command(
        "workbench.debug.action.focusRepl",
        "Debug: Focus on Debug Console View",
        Workbench,
        None,
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
pub(crate) fn match_label(label: &str, query: &str) -> Option<Vec<usize>> {
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
            "workbench.action.splitEditor" | "workbench.action.splitEditorRight" => {
                self.split_by_command(id);
            }
            "workbench.action.splitEditorDown" => drop(self.split_group(SplitDirection::Down)),
            "workbench.action.focusFirstEditorGroup" => drop(self.focus_group_index(0)),
            "workbench.action.focusSecondEditorGroup" => drop(self.focus_group_index(1)),
            "workbench.action.focusThirdEditorGroup" => drop(self.focus_group_index(2)),
            "workbench.action.focusNextGroup" => drop(self.focus_next_group(true)),
            "workbench.action.focusPreviousGroup" => drop(self.focus_next_group(false)),
            "workbench.action.focusLeftGroup" => drop(self.focus_group_toward(Direction::Left)),
            "workbench.action.focusRightGroup" => drop(self.focus_group_toward(Direction::Right)),
            "workbench.action.focusAboveGroup" => drop(self.focus_group_toward(Direction::Up)),
            "workbench.action.focusBelowGroup" => drop(self.focus_group_toward(Direction::Down)),
            "workbench.action.moveEditorToNextGroup" => drop(self.move_editor_to_group(true)),
            "workbench.action.moveEditorToPreviousGroup" => drop(self.move_editor_to_group(false)),
            "workbench.action.joinAllGroups" => drop(self.join_all_groups()),
            "workbench.action.closeGroup" => {
                let group = self.active_group();
                self.close_group(group)?;
            }
            "workbench.action.closeEditorsInGroup" => {
                let group = self.active_group();
                self.close_editors_in_group(group, false)?;
            }
            "workbench.action.previousEditor" => self.previous_tab(),
            "editor.action.toggleMinimap" => drop(self.toggle_minimap()),
            "workbench.action.debug.start" => {
                return self.start_debugging().map(|()| true).map_err(Error::Debug);
            }
            "workbench.action.debug.continue" => {
                return self
                    .continue_debugging()
                    .map(|()| true)
                    .map_err(Error::Debug);
            }
            "workbench.action.debug.stop" => {
                return self.stop_debugging().map(|()| true).map_err(Error::Debug);
            }
            "workbench.action.debug.stepOver" => return self.step_command(StepKind::Over),
            "workbench.action.debug.stepInto" => return self.step_command(StepKind::Into),
            "workbench.action.debug.stepOut" => return self.step_command(StepKind::Out),
            "workbench.action.debug.pause" => {
                return self.pause_debugging().map(|()| true).map_err(Error::Debug);
            }
            "workbench.debug.viewlet.action.removeAllWatchExpressions" => {
                self.remove_all_watches();
            }
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
    /// `workbench.extensions.enableExtension` and
    /// `workbench.extensions.disableExtension` take an extension id too; VS
    /// Code registers no such commands (its view runs the actions
    /// `extensions.enableGlobally` and `extensions.disableGlobally`,
    /// `src/vs/workbench/contrib/extensions/browser/extensionsActions.ts:1741-1850`),
    /// so these ids are rustode's, named like the install command.
    /// Returns `false` for an unknown command.
    ///
    /// # Errors
    /// When an extension command is refused (see
    /// [`Workspace::install_extension`]).
    pub fn execute_with(&mut self, id: &str, arg: &str) -> Result<bool, crate::ExtensionError> {
        match id {
            "workbench.extensions.installExtension" => self.install_extension(arg)?,
            "workbench.extensions.uninstallExtension" => self.uninstall_extension(arg)?,
            "workbench.extensions.enableExtension" => self.enable_extension(arg)?,
            "workbench.extensions.disableExtension" => self.disable_extension(arg)?,
            "workbench.extensions.search" => self.search_extensions(arg),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn step_command(&mut self, kind: StepKind) -> Result<bool, Error> {
        self.step_debugging(kind)
            .map(|()| true)
            .map_err(Error::Debug)
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
            "editor.action.triggerSuggest" => return self.request_completion(id),
            "editor.action.showHover" => {
                let cursor = self.document(id).map_or(0, crate::Document::cursor);
                return self.request_hover(id, cursor);
            }
            "editor.debug.action.toggleBreakpoint" => {
                let line = self.document(id).map_or(0, |d| d.cursor_position().line);
                self.toggle_breakpoint(id, line)?;
            }
            "editor.action.revealDefinition" => {
                let cursor = self.document(id).map_or(0, crate::Document::cursor);
                return self.request_definition(id, cursor);
            }
            "editor.action.goToReferences" => {
                let cursor = self.document(id).map_or(0, crate::Document::cursor);
                return self.request_references(id, cursor);
            }
            "editor.action.formatDocument" => return self.format_document(id),
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
    fn an_editor_group_key_shared_with_another_command_has_when_clauses_on_both() {
        for (i, a) in COMMANDS.iter().enumerate() {
            for b in &COMMANDS[i + 1..] {
                // Other pairs (Search's replace keys, Delete in two views) are
                // told apart by which part has focus, in the frontends.
                let editor_area = |c: &CommandInfo| {
                    c.id.contains("Group")
                        || c.id.contains("splitEditor")
                        || c.id == "workbench.action.closeActiveEditor"
                };
                if a.default_key.is_some()
                    && a.default_key == b.default_key
                    && (editor_area(a) || editor_area(b))
                {
                    assert!(
                        a.when.is_some() && b.when.is_some(),
                        "{} and {} share {:?} without `when` clauses",
                        a.id,
                        b.id,
                        a.default_key
                    );
                }
            }
        }
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
