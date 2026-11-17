//! UI-agnostic text buffer, selections, edits and undo. No I/O.

mod buffer;
mod editor;
mod history;
mod selection;

pub use buffer::Buffer;
pub use editor::{EditError, Editor, Range};
pub use history::EditKind;
pub use selection::{Position, Selection};
