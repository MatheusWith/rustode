//! UI-agnostic text buffer, selections, edits and undo. No I/O.

mod buffer;
mod editor;
mod history;

pub use buffer::Buffer;
pub use editor::{EditError, Editor, Range};
pub use history::EditKind;
