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

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rustode_workspace::Movement;

/// What a key asks the editor to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Type(String),
    Enter,
    Tab,
    DeleteLeft,
    DeleteRight,
    Move(Movement, bool),
    Page { up: bool, select: bool },
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
    NextTab,
    PreviousTab,
    CloseTab,
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
        (KeyCode::PageDown, true, false, false) => Some(Command::NextTab),
        (KeyCode::PageUp, true, false, false) => Some(Command::PreviousTab),
        (KeyCode::Char(c), true, _, false) => match (c.to_ascii_lowercase(), shift) {
            ('e', true) => Some(Command::FocusExplorer),
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
        KeyCode::Left if plain => Some(Command::Move(Movement::Left, shift)),
        KeyCode::Right if plain => Some(Command::Move(Movement::Right, shift)),
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
        KeyCode::Backspace if plain => Some(Command::DeleteLeft),
        KeyCode::Delete if plain => Some(Command::DeleteRight),
        KeyCode::Esc => Some(Command::Escape),
        KeyCode::F(5) => Some(Command::Reload),
        _ => None,
    }
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
            None,
            "word moves are not implemented yet"
        );
    }
}
