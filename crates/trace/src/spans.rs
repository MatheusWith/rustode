//! Shared span names and thin constructors, so every binary traces the same
//! operations under the same names and fields.
//!
//! Enter the returned span (`let _guard = span.enter();`) or attach it to a
//! future with `tracing::Instrument`. Frequent operations (renders, child
//! messages) use the `TRACE` level; edits and file operations use `DEBUG`.

use std::path::Path;

use tracing::Span;

/// Span name for a buffer edit.
pub const EDIT: &str = "edit";
/// Span name for producing one frame.
pub const RENDER: &str = "render";
/// Span name for a file-system operation.
pub const FILE_OP: &str = "file_op";
/// Span name for one message to or from a child process.
pub const CHILD_MESSAGE: &str = "child_message";

/// Direction of a child-process message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Sent by us to the child.
    Outgoing,
    /// Received from the child.
    Incoming,
}

impl Direction {
    /// Lower-case label recorded in the span.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Outgoing => "out",
            Self::Incoming => "in",
        }
    }
}

/// Span for an edit to the buffer identified by `buffer`.
#[must_use]
pub fn edit(buffer: &str) -> Span {
    tracing::debug_span!(EDIT, buffer)
}

/// Span for rendering frame number `frame`.
#[must_use]
pub fn render(frame: u64) -> Span {
    tracing::trace_span!(RENDER, frame)
}

/// Span for the file operation `op` (for example `read`, `write`) on `path`.
#[must_use]
pub fn file_op(op: &str, path: &Path) -> Span {
    tracing::debug_span!(FILE_OP, op, path = %path.display())
}

/// Span for a message of kind `method` exchanged with the child `process`.
#[must_use]
pub fn child_message(process: &str, direction: Direction, method: &str) -> Span {
    tracing::trace_span!(
        CHILD_MESSAGE,
        process,
        direction = direction.as_str(),
        method
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;

    /// Runs `make` under a subscriber that enables every level.
    fn metadata_of(make: impl FnOnce() -> Span) -> (&'static str, Level, Vec<&'static str>) {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(Level::TRACE)
            .with_writer(std::io::sink)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let span = make();
            let meta = span.metadata().expect("span is enabled");
            let fields = meta.fields().iter().map(|f| f.name()).collect();
            (meta.name(), *meta.level(), fields)
        })
    }

    #[test]
    fn edit_span() {
        let (name, level, fields) = metadata_of(|| edit("main.rs"));
        assert_eq!((name, level), (EDIT, Level::DEBUG));
        assert_eq!(fields, ["buffer"]);
    }

    #[test]
    fn render_span() {
        let (name, level, fields) = metadata_of(|| render(42));
        assert_eq!((name, level), (RENDER, Level::TRACE));
        assert_eq!(fields, ["frame"]);
    }

    #[test]
    fn file_op_span() {
        let (name, level, fields) = metadata_of(|| file_op("read", Path::new("a.txt")));
        assert_eq!((name, level), (FILE_OP, Level::DEBUG));
        assert_eq!(fields, ["op", "path"]);
    }

    #[test]
    fn child_message_span() {
        let (name, level, fields) =
            metadata_of(|| child_message("ext-host", Direction::Incoming, "ready"));
        assert_eq!((name, level), (CHILD_MESSAGE, Level::TRACE));
        assert_eq!(fields, ["process", "direction", "method"]);
        assert_eq!(Direction::Outgoing.as_str(), "out");
    }
}
