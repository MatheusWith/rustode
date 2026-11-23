//! UI-agnostic text buffer, selections, edits and undo. No I/O.

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::unimplemented,
        clippy::todo
    )
)]

mod buffer;
mod editor;
mod history;
mod selection;

pub use buffer::{Buffer, ByteEdit, BytePosition};
pub use editor::{CharEdit, EditError, Editor, Movement, Range};
pub use history::EditKind;
pub use selection::{Position, Selection, Selections};
