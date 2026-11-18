//! UI-agnostic text buffer, selections, edits and undo. No I/O.

mod buffer;
mod editor;
mod history;
mod selection;

pub use buffer::{Buffer, ByteEdit, BytePosition};
pub use editor::{EditError, Editor, Movement, Range};
pub use history::EditKind;
pub use selection::{Position, Selection};
