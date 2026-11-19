//! Terminal keys to editor commands, using VS Code's default Linux bindings
//! where a terminal can deliver them. Add cursor above/below follows
//! `InsertCursorAbove`/`InsertCursorBelow`
//! (`src/vs/editor/contrib/multicursor/browser/multicursor.ts:46-60`, `:98-112`):
//! Shift+Alt+Up/Down, or Ctrl+Shift+Up/Down. Workbench keys: Ctrl+Shift+E
//! shows the explorer (`src/vs/workbench/contrib/files/browser/explorerViewlet.ts:271`),
//! F6 moves focus to the next part (`src/vs/workbench/browser/actions/navigationActions.ts:362-367`)
//! and works in every terminal, Ctrl+PageDown/PageUp switch tabs
//! (`src/vs/workbench/browser/parts/editor/editorActions.ts:1277`, `:1325`)
//! and Ctrl+W closes one (`src/vs/workbench/browser/parts/editor/editorCommands.ts:841`).
//! Ctrl+Shift+X shows the Extensions view
//! (`src/vs/workbench/contrib/extensions/browser/extensions.contribution.ts:121`),
//! Ctrl+Shift+F the Search view
//! (`src/vs/workbench/contrib/search/browser/searchActionsFind.ts:239-242`).
//! Ctrl+P opens quick open, Ctrl+Shift+P or F1 the command palette,
//! Ctrl+G go to line and Ctrl+Shift+M toggles the problems panel (see
//! `rustode_workspace`'s command registry). Terminals without an extended
//! keyboard protocol send Ctrl+Shift+M as Enter; the palette and a click on
//! the status bar's counts toggle the panel too.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rustode_workspace::{Chord, Movement};

/// What a key asks the editor to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Type(String),
    Enter,
    Tab,
    DeleteLeft,
    DeleteRight,
    DeleteWordLeft,
    DeleteWordRight,
    Move(Movement, bool),
    Page {
        up: bool,
        select: bool,
    },
    Undo,
    Redo,
    Save,
    Quit,
    SelectAll,
    AddNextOccurrence,
    AddCursorAbove,
    AddCursorBelow,
    Escape,
    Reload,
    FocusExplorer,
    FocusNextPart,
    ShowExtensions,
    ShowSearch,
    /// `workbench.action.replaceInFiles`: the Search view with its replace field.
    ShowReplace,
    /// `search.action.replaceAll`.
    SearchReplaceAll,
    /// `search.action.replace`: the selected match.
    SearchReplace,
    /// `search.action.replaceAllInFile`: the selected file.
    SearchReplaceFile,
    /// `search.action.remove`: dismiss the selected match or file.
    SearchRemove,
    /// `rustode.search.undoReplace`.
    SearchUndoReplace,
    NextTab,
    PreviousTab,
    CloseTab,
    QuickOpen,
    ShowCommands,
    GotoLine,
    Find,
    Replace,
    FindNext,
    FindPrevious,
    ToggleProblems,
    ChangeLanguage,
    DebugStart,
    DebugContinue,
    DebugStop,
    ToggleBreakpoint,
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
    /// Enter opens and moves focus to the editor; Space keeps focus here.
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

/// Commands that work whichever part has focus.
pub(crate) fn global(key: KeyEvent) -> Option<Command> {
    let (ctrl, shift, alt) = split(key.modifiers);
    match (key.code, ctrl, shift, alt) {
        (KeyCode::F(6), false, false, false) => Some(Command::FocusNextPart),
        (KeyCode::F(1), false, false, false) => Some(Command::ShowCommands),
        (KeyCode::F(3), false, false, false) => Some(Command::FindNext),
        (KeyCode::F(3), false, true, false) => Some(Command::FindPrevious),
        (KeyCode::PageDown, true, false, false) => Some(Command::NextTab),
        (KeyCode::PageUp, true, false, false) => Some(Command::PreviousTab),
        (KeyCode::Char(c), true, _, false) => match (c.to_ascii_lowercase(), shift) {
            ('e', true) => Some(Command::FocusExplorer),
            ('x', true) => Some(Command::ShowExtensions),
            ('m', true) => Some(Command::ToggleProblems),
            ('f', true) => Some(Command::ShowSearch),
            ('h', true) => Some(Command::ShowReplace),
            ('p', false) => Some(Command::QuickOpen),
            ('p', true) => Some(Command::ShowCommands),
            ('g', false) => Some(Command::GotoLine),
            ('f', false) => Some(Command::Find),
            ('h', false) => Some(Command::Replace),
            ('w', false) => Some(Command::CloseTab),
            ('s', false) => Some(Command::Save),
            ('q', false) => Some(Command::Quit),
            _ => None,
        },
        _ => None,
    }
}

/// The explorer's own keys.
pub(crate) fn explorer_key(key: KeyEvent) -> Option<ExplorerKey> {
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return None;
    }
    Some(match key.code {
        KeyCode::Up => ExplorerKey::Up,
        KeyCode::Down => ExplorerKey::Down,
        KeyCode::PageUp => ExplorerKey::PageUp,
        KeyCode::PageDown => ExplorerKey::PageDown,
        KeyCode::Home => ExplorerKey::Home,
        KeyCode::End => ExplorerKey::End,
        KeyCode::Enter => ExplorerKey::Open { keep_focus: false },
        KeyCode::Char(' ') => ExplorerKey::Open { keep_focus: true },
        KeyCode::Left => ExplorerKey::Collapse,
        KeyCode::Right => ExplorerKey::Expand,
        KeyCode::Esc => ExplorerKey::Leave,
        _ => return None,
    })
}

/// VS Code's id for each command a key can run, so `keybindings.json` can
/// bind or remove it (`cursorDown`, `deleteLeft`, …).
pub(crate) fn id_of(command: &Command) -> Option<&'static str> {
    Some(match command {
        Command::Save => "workbench.action.files.save",
        Command::Quit => "workbench.action.quit",
        Command::CloseTab => "workbench.action.closeActiveEditor",
        Command::NextTab => "workbench.action.nextEditor",
        Command::PreviousTab => "workbench.action.previousEditor",
        Command::FocusExplorer => "workbench.view.explorer",
        Command::FocusNextPart => "workbench.action.focusNextPart",
        Command::ShowExtensions => "workbench.view.extensions",
        Command::ShowSearch => "workbench.action.findInFiles",
        Command::ShowReplace => "workbench.action.replaceInFiles",
        Command::SearchReplaceAll => "search.action.replaceAll",
        Command::SearchReplace => "search.action.replace",
        Command::SearchReplaceFile => "search.action.replaceAllInFile",
        Command::SearchRemove => "search.action.remove",
        Command::SearchUndoReplace => "rustode.search.undoReplace",
        Command::QuickOpen => "workbench.action.quickOpen",
        Command::ShowCommands => "workbench.action.showCommands",
        Command::GotoLine => "workbench.action.gotoLine",
        Command::Find => "actions.find",
        Command::Replace => "editor.action.startFindReplaceAction",
        Command::FindNext => "editor.action.nextMatchFindAction",
        Command::FindPrevious => "editor.action.previousMatchFindAction",
        Command::ToggleProblems => "workbench.actions.view.problems",
        Command::ChangeLanguage => "workbench.action.editor.changeLanguageMode",
        Command::DebugStart => "workbench.action.debug.start",
        Command::DebugContinue => "workbench.action.debug.continue",
        Command::DebugStop => "workbench.action.debug.stop",
        Command::ToggleBreakpoint => "editor.debug.action.toggleBreakpoint",
        Command::Undo => "undo",
        Command::Redo => "redo",
        Command::SelectAll => "editor.action.selectAll",
        Command::AddNextOccurrence => "editor.action.addSelectionToNextFindMatch",
        Command::AddCursorAbove => "editor.action.insertCursorAbove",
        Command::AddCursorBelow => "editor.action.insertCursorBelow",
        Command::Escape => "cancelSelection",
        Command::Tab => "tab",
        Command::DeleteLeft => "deleteLeft",
        Command::DeleteRight => "deleteRight",
        Command::DeleteWordLeft => "deleteWordLeft",
        Command::DeleteWordRight => "deleteWordRight",
        Command::Move(movement, select) => move_id(*movement, *select)?,
        Command::Page { up, select } => match (up, select) {
            (true, false) => "cursorPageUp",
            (true, true) => "cursorPageUpSelect",
            (false, false) => "cursorPageDown",
            (false, true) => "cursorPageDownSelect",
        },
        Command::Type(_) | Command::Enter | Command::Reload => return None,
    })
}

const MOVES: [(Movement, &str, &str); 10] = [
    (Movement::WordLeft, "cursorWordLeft", "cursorWordLeftSelect"),
    (
        Movement::WordRight,
        "cursorWordEndRight",
        "cursorWordEndRightSelect",
    ),
    (Movement::Left, "cursorLeft", "cursorLeftSelect"),
    (Movement::Right, "cursorRight", "cursorRightSelect"),
    (Movement::Up, "cursorUp", "cursorUpSelect"),
    (Movement::Down, "cursorDown", "cursorDownSelect"),
    (Movement::LineStart, "cursorHome", "cursorHomeSelect"),
    (Movement::LineEnd, "cursorEnd", "cursorEndSelect"),
    (Movement::DocumentStart, "cursorTop", "cursorTopSelect"),
    (Movement::DocumentEnd, "cursorBottom", "cursorBottomSelect"),
];

fn move_id(movement: Movement, select: bool) -> Option<&'static str> {
    MOVES
        .iter()
        .find(|(m, _, _)| *m == movement)
        .map(|(_, plain, selecting)| if select { *selecting } else { *plain })
}

/// The command for a VS Code id, for ids a key can also run.
pub(crate) fn from_id(id: &str) -> Option<Command> {
    if id == "removeSecondaryCursors" {
        return Some(Command::Escape);
    }
    if let Some((movement, plain, _)) = MOVES.iter().find(|(_, p, s)| *p == id || *s == id) {
        return Some(Command::Move(*movement, *plain != id));
    }
    let all = [
        Command::Save,
        Command::Quit,
        Command::CloseTab,
        Command::NextTab,
        Command::PreviousTab,
        Command::FocusExplorer,
        Command::FocusNextPart,
        Command::ShowExtensions,
        Command::ShowSearch,
        Command::ShowReplace,
        Command::SearchReplaceAll,
        Command::SearchReplace,
        Command::SearchReplaceFile,
        Command::SearchRemove,
        Command::SearchUndoReplace,
        Command::QuickOpen,
        Command::ShowCommands,
        Command::GotoLine,
        Command::Find,
        Command::Replace,
        Command::FindNext,
        Command::FindPrevious,
        Command::ToggleProblems,
        Command::ChangeLanguage,
        Command::DebugStart,
        Command::DebugContinue,
        Command::DebugStop,
        Command::ToggleBreakpoint,
        Command::Undo,
        Command::Redo,
        Command::SelectAll,
        Command::AddNextOccurrence,
        Command::AddCursorAbove,
        Command::AddCursorBelow,
        Command::Escape,
        Command::Tab,
        Command::DeleteLeft,
        Command::DeleteRight,
        Command::DeleteWordLeft,
        Command::DeleteWordRight,
        Command::Page {
            up: true,
            select: false,
        },
        Command::Page {
            up: true,
            select: true,
        },
        Command::Page {
            up: false,
            select: false,
        },
        Command::Page {
            up: false,
            select: true,
        },
    ];
    all.into_iter().find(|command| id_of(command) == Some(id))
}

/// Whether `key` is the chord, with key names as `keybindings.json` writes
/// them (`ctrl+shift+p`, `pagedown`, `f5`); an uppercase letter implies Shift.
pub(crate) fn matches_chord(key: KeyEvent, chord: &Chord) -> bool {
    let (ctrl, mut shift, alt) = split(key.modifiers);
    let meta = key.modifiers.contains(KeyModifiers::SUPER);
    let name = match key.code {
        KeyCode::Char(' ') => "space".to_owned(),
        KeyCode::Char(c) => {
            shift |= c.is_uppercase();
            c.to_lowercase().collect()
        }
        KeyCode::Enter => "enter".to_owned(),
        KeyCode::Tab => "tab".to_owned(),
        KeyCode::Backspace => "backspace".to_owned(),
        KeyCode::Delete => "delete".to_owned(),
        KeyCode::Insert => "insert".to_owned(),
        KeyCode::Esc => "escape".to_owned(),
        KeyCode::Up => "up".to_owned(),
        KeyCode::Down => "down".to_owned(),
        KeyCode::Left => "left".to_owned(),
        KeyCode::Right => "right".to_owned(),
        KeyCode::Home => "home".to_owned(),
        KeyCode::End => "end".to_owned(),
        KeyCode::PageUp => "pageup".to_owned(),
        KeyCode::PageDown => "pagedown".to_owned(),
        KeyCode::F(n) => format!("f{n}"),
        _ => return false,
    };
    chord.key == name
        && chord.ctrl == ctrl
        && chord.shift == shift
        && chord.alt == alt
        && chord.meta == meta
}

/// Ctrl, Shift and Alt.
fn split(modifiers: KeyModifiers) -> (bool, bool, bool) {
    (
        modifiers.contains(KeyModifiers::CONTROL),
        modifiers.contains(KeyModifiers::SHIFT),
        modifiers.contains(KeyModifiers::ALT),
    )
}

/// The command for `key` while the editor has focus, if it has one.
pub(crate) fn command(key: KeyEvent) -> Option<Command> {
    if let Some(command) = global(key) {
        return Some(command);
    }
    let (ctrl, shift, alt) = split(key.modifiers);
    let plain = !ctrl && !alt;
    match key.code {
        KeyCode::Char(c) => char_command(c, key.modifiers),
        KeyCode::Up => vertical(true, key.modifiers),
        KeyCode::Down => vertical(false, key.modifiers),
        KeyCode::Left | KeyCode::Right | KeyCode::Backspace | KeyCode::Delete => {
            horizontal(key.code, key.modifiers)
        }
        KeyCode::Home if !alt => Some(Command::Move(
            if ctrl {
                Movement::DocumentStart
            } else {
                Movement::LineStart
            },
            shift,
        )),
        KeyCode::End if !alt => Some(Command::Move(
            if ctrl {
                Movement::DocumentEnd
            } else {
                Movement::LineEnd
            },
            shift,
        )),
        KeyCode::PageUp if plain => Some(Command::Page {
            up: true,
            select: shift,
        }),
        KeyCode::PageDown if plain => Some(Command::Page {
            up: false,
            select: shift,
        }),
        KeyCode::Enter if plain => Some(Command::Enter),
        KeyCode::Tab if plain && !shift => Some(Command::Tab),
        KeyCode::Esc => Some(Command::Escape),
        KeyCode::F(5) if shift => Some(Command::DebugStop),
        KeyCode::F(5) => Some(Command::Reload),
        KeyCode::F(9) => Some(Command::ToggleBreakpoint),
        _ => None,
    }
}

/// Left/Right move by char, or by word with Ctrl; Backspace/Delete delete
/// a char, or a word with Ctrl (`cursorWordLeft`, `cursorWordEndRight`,
/// `deleteWordLeft`, `deleteWordRight`).
fn horizontal(code: KeyCode, modifiers: KeyModifiers) -> Option<Command> {
    let (ctrl, shift, alt) = split(modifiers);
    if alt {
        return None;
    }
    Some(match (code, ctrl) {
        (KeyCode::Left, false) => Command::Move(Movement::Left, shift),
        (KeyCode::Right, false) => Command::Move(Movement::Right, shift),
        (KeyCode::Left, true) => Command::Move(Movement::WordLeft, shift),
        (KeyCode::Right, true) => Command::Move(Movement::WordRight, shift),
        (KeyCode::Backspace, false) => Command::DeleteLeft,
        (KeyCode::Delete, false) => Command::DeleteRight,
        (KeyCode::Backspace, true) => Command::DeleteWordLeft,
        (KeyCode::Delete, true) => Command::DeleteWordRight,
        _ => return None,
    })
}

fn char_command(c: char, modifiers: KeyModifiers) -> Option<Command> {
    let (ctrl, shift, alt) = split(modifiers);
    match (ctrl, alt) {
        // Ctrl+Alt is AltGr on many keyboard layouts: it types a char.
        (false, false) | (true, true) => Some(Command::Type(c.to_string())),
        (false, true) => None,
        (true, false) => match (c.to_ascii_lowercase(), shift) {
            ('z', false) => Some(Command::Undo),
            ('z', true) | ('y', false) => Some(Command::Redo),
            ('a', false) => Some(Command::SelectAll),
            ('d', false) => Some(Command::AddNextOccurrence),
            _ => None,
        },
    }
}

fn vertical(up: bool, modifiers: KeyModifiers) -> Option<Command> {
    match split(modifiers) {
        (false, true, true) | (true, true, false) => Some(if up {
            Command::AddCursorAbove
        } else {
            Command::AddCursorBelow
        }),
        (false, select, false) => Some(Command::Move(
            if up { Movement::Up } else { Movement::Down },
            select,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Option<Command> {
        command(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn shortcuts_follow_vs_code_on_linux() {
        let ctrl = KeyModifiers::CONTROL;
        let shift = KeyModifiers::SHIFT;
        assert_eq!(key(KeyCode::Char('s'), ctrl), Some(Command::Save));
        assert_eq!(key(KeyCode::Char('Z'), ctrl | shift), Some(Command::Redo));
        assert_eq!(
            key(KeyCode::Up, shift | KeyModifiers::ALT),
            Some(Command::AddCursorAbove)
        );
        assert_eq!(
            key(KeyCode::Down, ctrl | shift),
            Some(Command::AddCursorBelow)
        );
        assert_eq!(
            key(KeyCode::Home, ctrl | shift),
            Some(Command::Move(Movement::DocumentStart, true))
        );
        assert_eq!(
            key(KeyCode::Char('é'), ctrl | KeyModifiers::ALT),
            Some(Command::Type("é".into()))
        );
        assert_eq!(key(KeyCode::Char('x'), KeyModifiers::ALT), None);
        assert_eq!(
            key(KeyCode::Left, ctrl),
            Some(Command::Move(Movement::WordLeft, false))
        );
        assert_eq!(key(KeyCode::Backspace, ctrl), Some(Command::DeleteWordLeft));
    }
}
