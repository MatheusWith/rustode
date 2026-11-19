//! Keyboard input to workspace commands, with VS Code's default Linux
//! keybindings (Code - OSS, studied and reimplemented; see `ATTRIBUTION.md`):
//!
//! - cursor moves, with Shift to select: `src/vs/editor/browser/coreCommands.ts:702-752`
//!   (Left, Right), `:768-856` (Up, Down), `:804-888` (Page Up, Page Down),
//!   `:1012-1025` (Home), `:1115-1145` (End), `:1247-1303` (Ctrl+Home, Ctrl+End);
//! - Escape `:1783-1811`, Tab `:2035`, Backspace and Shift+Backspace
//!   `:2055-2056`, Delete `:2080`;
//! - Ctrl+Z `src/vs/editor/browser/editorExtensions.ts:651`, Ctrl+Y and
//!   Ctrl+Shift+Z `:678-680`, Ctrl+A `:708`;
//! - Ctrl+D, and Shift+Alt+Up/Down or Ctrl+Shift+Up/Down to add cursors:
//!   `src/vs/editor/contrib/multicursor/browser/multicursor.ts:55-58`,
//!   `:107-110`, `:734`.
//!
//! Workbench keys work whichever part has focus:
//!
//! - Ctrl+S saves (`src/vs/workbench/contrib/files/browser/fileCommands.ts:504`);
//! - Ctrl+PageDown and Ctrl+PageUp switch to the next and previous tab
//!   (`src/vs/workbench/browser/parts/editor/editorActions.ts:1277`, `:1325`);
//! - Ctrl+Tab and Ctrl+Shift+Tab switch to the previously and the least
//!   recently used tab (`:1868-1905`);
//! - Ctrl+W closes the active tab
//!   (`src/vs/workbench/browser/parts/editor/editorCommands.ts:841`);
//! - Ctrl+B shows or hides the sidebar
//!   (`src/vs/workbench/browser/actions/layoutActions.ts:308-311`);
//! - Ctrl+Shift+E focuses the explorer
//!   (`src/vs/workbench/contrib/files/browser/explorerViewlet.ts:271`);
//! - F6 and Shift+F6 move focus to the next and the previous part
//!   (`src/vs/workbench/browser/actions/navigationActions.ts:358-388`);
//! - Ctrl+M turns "Tab Moves Focus" on and off
//!   (`src/vs/editor/contrib/toggleTabFocusMode/browser/toggleTabFocusMode.ts:15-43`);
//! - Ctrl+Shift+M toggles the problems panel
//!   (`src/vs/workbench/contrib/markers/browser/markers.contribution.ts:150-154`);
//! - Ctrl+Shift+F shows the Search view
//!   (`src/vs/workbench/contrib/search/browser/searchActionsFind.ts:239-242`);
//! - Ctrl+Q quits (`src/vs/workbench/electron-browser/desktop.contribution.ts:82-91`);
//! - Ctrl+P opens quick open, and in it selects the next row
//!   (`src/vs/workbench/browser/actions/quickAccessActions.ts:24-28`, `:75-84`);
//!   Ctrl+Shift+P or F1 the command palette
//!   (`src/vs/workbench/contrib/quickaccess/browser/commandsQuickAccess.ts:275-285`);
//!   Ctrl+G go to line
//!   (`src/vs/workbench/contrib/codeEditor/browser/quickaccess/gotoLineQuickAccess.ts:71-84`);
//! - Ctrl+F finds, Ctrl+H replaces, F3 and Shift+F3 go to the next and
//!   previous match (`src/vs/editor/contrib/find/browser/findController.ts:572`,
//!   `:783-809`, `:1010`).
//!
//! The quick input and the find widget handle their own keys (`quick_input.rs`,
//! `find.rs`).
//!
//! In the explorer, as in VS Code's lists
//! (`src/vs/workbench/browser/actions/listCommands.ts:336-365`, `:461-464`,
//! `:559-562`, `:754-757`): Up, Down, Page Up, Page Down, Home and End move
//! the selection, Left collapses a folder or goes to its parent, Right
//! expands one or goes to its first child, Enter opens and moves focus to
//! the editor, Space opens and keeps it; Escape goes back to the editor, as
//! in the TUI.
//!
//! User keybindings (`keybindings.json`) are not applied yet: the workspace
//! has no command dispatcher for frontends.

use eframe::egui::{Event, ImeEvent, Key, Modifiers};
use rustode_workspace::Movement;

/// What a key press asks the workspace to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Type(String),
    DeleteLeft,
    DeleteRight,
    Enter,
    Tab,
    /// A cursor move; `true` extends the selection (Shift).
    Move(Movement, bool),
    Undo,
    Redo,
    SelectAll,
    Escape,
    AddNextOccurrence,
    AddCursorAbove,
    AddCursorBelow,
}

/// The command for one input event; `page` is the Page Up/Down distance in
/// lines.
pub(crate) fn command(event: &Event, page: usize) -> Option<Command> {
    match event {
        Event::Text(text) | Event::Ime(ImeEvent::Commit(text)) if !text.is_empty() => {
            Some(Command::Type(text.clone()))
        }
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => key_command(*key, *modifiers, page),
        _ => None,
    }
}

fn key_command(key: Key, modifiers: Modifiers, page: usize) -> Option<Command> {
    let ctrl = modifiers.command || modifiers.ctrl;
    let shift = modifiers.shift;
    match (ctrl, modifiers.alt) {
        (false, false) => plain_key(key, shift, page),
        (true, false) => ctrl_key(key, shift),
        (false, true) if shift => match key {
            Key::ArrowUp => Some(Command::AddCursorAbove),
            Key::ArrowDown => Some(Command::AddCursorBelow),
            _ => None,
        },
        _ => None,
    }
}

fn plain_key(key: Key, shift: bool, page: usize) -> Option<Command> {
    let movement = match key {
        Key::ArrowLeft => Movement::Left,
        Key::ArrowRight => Movement::Right,
        Key::ArrowUp => Movement::Up,
        Key::ArrowDown => Movement::Down,
        Key::Home => Movement::LineStart,
        Key::End => Movement::LineEnd,
        Key::PageUp => Movement::PageUp(page),
        Key::PageDown => Movement::PageDown(page),
        Key::Backspace => return Some(Command::DeleteLeft),
        Key::Delete if !shift => return Some(Command::DeleteRight),
        Key::Enter => return Some(Command::Enter),
        Key::Tab if !shift => return Some(Command::Tab),
        Key::Escape => return Some(Command::Escape),
        _ => return None,
    };
    Some(Command::Move(movement, shift))
}

fn ctrl_key(key: Key, shift: bool) -> Option<Command> {
    Some(match (key, shift) {
        (Key::Home, _) => Command::Move(Movement::DocumentStart, shift),
        (Key::End, _) => Command::Move(Movement::DocumentEnd, shift),
        (Key::ArrowUp, true) => Command::AddCursorAbove,
        (Key::ArrowDown, true) => Command::AddCursorBelow,
        (Key::Z, false) => Command::Undo,
        (Key::Z, true) | (Key::Y, false) => Command::Redo,
        (Key::A, false) => Command::SelectAll,
        (Key::D, false) => Command::AddNextOccurrence,
        _ => return None,
    })
}

/// What a workbench key asks for, whichever part has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Workbench {
    Save,
    NextTab,
    PreviousTab,
    /// Ctrl+Tab: the tab used before the active one.
    RecentTab,
    /// Ctrl+Shift+Tab: the tab used longest ago.
    LeastRecentTab,
    CloseTab,
    ToggleSidebar,
    FocusExplorer,
    FocusNextPart,
    FocusPreviousPart,
    /// Ctrl+M: whether Tab moves focus instead of indenting.
    ToggleTabMovesFocus,
    /// Ctrl+Shift+X: the Extensions view in the sidebar.
    ShowExtensions,
    /// Ctrl+Shift+M: the problems panel.
    ToggleProblems,
    /// Ctrl+Shift+F: the Search view in the sidebar.
    ShowSearch,
    /// Ctrl+Shift+H: the Search view with its replace field.
    ShowReplace,
    /// `search.action.replaceAll`: asks to confirm, then replaces.
    SearchReplaceAll,
    /// Ctrl+Shift+1, `search.action.replace`: the selected result.
    SearchReplace,
    /// `search.action.replaceAllInFile`: the selected result's file.
    SearchReplaceFile,
    /// `search.action.remove`: dismisses the selected result.
    SearchRemove,
    /// `rustode.search.undoReplace`.
    SearchUndoReplace,
    /// Ctrl+Q: closes the window; with unsaved changes, only when asked twice.
    Quit,
    /// Ctrl+P: quick open.
    QuickOpen,
    /// Ctrl+Shift+P or F1: the command palette.
    ShowCommands,
    /// Ctrl+G: go to a line.
    GotoLine,
    /// Ctrl+F: the find widget.
    Find,
    /// Ctrl+H: the find widget with its replace field.
    Replace,
    /// F3: the next match of the last search.
    FindNext,
    /// Shift+F3: the previous match.
    FindPrevious,
    /// F5: start debugging, or continue a stopped program.
    DebugKey,
    /// Shift+F5: stop debugging.
    DebugStop,
    /// F9: a breakpoint on the cursor's line.
    ToggleBreakpoint,
}

impl Workbench {
    /// The workbench command a registry id names, for the command palette.
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "workbench.action.files.save" => Self::Save,
            "workbench.action.quit" => Self::Quit,
            "workbench.action.closeActiveEditor" => Self::CloseTab,
            "workbench.view.explorer" => Self::FocusExplorer,
            "workbench.view.extensions" => Self::ShowExtensions,
            "workbench.action.focusNextPart" => Self::FocusNextPart,
            "workbench.action.findInFiles" => Self::ShowSearch,
            "workbench.action.replaceInFiles" => Self::ShowReplace,
            "search.action.replaceAll" => Self::SearchReplaceAll,
            "search.action.replace" => Self::SearchReplace,
            "search.action.replaceAllInFile" => Self::SearchReplaceFile,
            "search.action.remove" => Self::SearchRemove,
            "rustode.search.undoReplace" => Self::SearchUndoReplace,
            "workbench.actions.view.problems" => Self::ToggleProblems,
            "workbench.action.quickOpen" => Self::QuickOpen,
            "workbench.action.showCommands" => Self::ShowCommands,
            "workbench.action.gotoLine" => Self::GotoLine,
            "actions.find" => Self::Find,
            "editor.action.startFindReplaceAction" => Self::Replace,
            "editor.action.nextMatchFindAction" => Self::FindNext,
            "editor.action.previousMatchFindAction" => Self::FindPrevious,
            _ => return None,
        })
    }
}

/// The workbench command for one input event, if it is a workbench key.
pub(crate) fn workbench(event: &Event) -> Option<Workbench> {
    let Event::Key {
        key,
        pressed: true,
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    let ctrl = modifiers.command || modifiers.ctrl;
    Some(match (key, ctrl, modifiers.shift, modifiers.alt) {
        (Key::S, true, false, false) => Workbench::Save,
        (Key::PageDown, true, false, false) => Workbench::NextTab,
        (Key::PageUp, true, false, false) => Workbench::PreviousTab,
        (Key::Tab, true, false, false) => Workbench::RecentTab,
        (Key::Tab, true, true, false) => Workbench::LeastRecentTab,
        (Key::W, true, false, false) => Workbench::CloseTab,
        (Key::B, true, false, false) => Workbench::ToggleSidebar,
        (Key::E, true, true, false) => Workbench::FocusExplorer,
        (Key::X, true, true, false) => Workbench::ShowExtensions,
        (Key::M, true, true, false) => Workbench::ToggleProblems,
        (Key::F, true, true, false) => Workbench::ShowSearch,
        (Key::H, true, true, false) => Workbench::ShowReplace,
        (Key::Num1, true, true, false) => Workbench::SearchReplace,
        (Key::F6, false, false, false) => Workbench::FocusNextPart,
        (Key::F6, false, true, false) => Workbench::FocusPreviousPart,
        (Key::M, true, false, false) => Workbench::ToggleTabMovesFocus,
        (Key::Q, true, false, false) => Workbench::Quit,
        (Key::P, true, false, false) => Workbench::QuickOpen,
        (Key::P, true, true, false) | (Key::F1, false, false, false) => Workbench::ShowCommands,
        (Key::G, true, false, false) => Workbench::GotoLine,
        (Key::F, true, false, false) => Workbench::Find,
        (Key::H, true, false, false) => Workbench::Replace,
        (Key::F3, false, false, false) => Workbench::FindNext,
        (Key::F3, false, true, false) => Workbench::FindPrevious,
        (Key::F5, false, false, false) => Workbench::DebugKey,
        (Key::F5, false, true, false) => Workbench::DebugStop,
        (Key::F9, false, false, false) => Workbench::ToggleBreakpoint,
        _ => return None,
    })
}

/// What a key does while the explorer has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExplorerKey {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    /// Enter opens and moves focus to the editor; Space keeps it here.
    Open {
        keep_focus: bool,
    },
    /// Left: collapse a folder, or go to the parent.
    Collapse,
    /// Right: expand a folder, or go to its first child.
    Expand,
    /// Escape: back to the editor.
    Leave,
}

/// The explorer's command for one input event.
pub(crate) fn explorer_key(event: &Event) -> Option<ExplorerKey> {
    let Event::Key {
        key,
        pressed: true,
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    if modifiers.any() {
        return None;
    }
    Some(match key {
        Key::ArrowUp => ExplorerKey::Up,
        Key::ArrowDown => ExplorerKey::Down,
        Key::PageUp => ExplorerKey::PageUp,
        Key::PageDown => ExplorerKey::PageDown,
        Key::Home => ExplorerKey::Home,
        Key::End => ExplorerKey::End,
        Key::Enter => ExplorerKey::Open { keep_focus: false },
        Key::Space => ExplorerKey::Open { keep_focus: true },
        Key::ArrowLeft => ExplorerKey::Collapse,
        Key::ArrowRight => ExplorerKey::Expand,
        Key::Escape => ExplorerKey::Leave,
        _ => return None,
    })
}
