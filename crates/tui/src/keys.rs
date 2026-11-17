//! Terminal keys to editor commands, using VS Code's default Linux bindings
//! where a terminal can deliver them. Add cursor above/below follows
//! `InsertCursorAbove`/`InsertCursorBelow`
//! (`src/vs/editor/contrib/multicursor/browser/multicursor.ts:46-60`, `:98-112`):
//! Shift+Alt+Up/Down, or Ctrl+Shift+Up/Down.

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
}

/// Ctrl, Shift and Alt.
fn split(modifiers: KeyModifiers) -> (bool, bool, bool) {
    (
        modifiers.contains(KeyModifiers::CONTROL),
        modifiers.contains(KeyModifiers::SHIFT),
        modifiers.contains(KeyModifiers::ALT),
    )
}

/// The command for `key`, if it has one.
pub(crate) fn command(key: KeyEvent) -> Option<Command> {
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
            ('s', false) => Some(Command::Save),
            ('z', false) => Some(Command::Undo),
            ('z', true) | ('y', false) => Some(Command::Redo),
            ('q', false) => Some(Command::Quit),
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
