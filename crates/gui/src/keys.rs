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
//! - Ctrl+S `src/vs/workbench/contrib/files/browser/fileCommands.ts:504`;
//! - Ctrl+D, and Shift+Alt+Up/Down or Ctrl+Shift+Up/Down to add cursors:
//!   `src/vs/editor/contrib/multicursor/browser/multicursor.ts:55-58`,
//!   `:107-110`, `:734`.
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
    Save,
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
        (Key::S, false) => Command::Save,
        (Key::A, false) => Command::SelectAll,
        (Key::D, false) => Command::AddNextOccurrence,
        _ => return None,
    })
}
