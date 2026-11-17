//! Undo/redo history with VS Code's grouping of typing into undo steps.
//!
//! The grouping rule was studied in VS Code (`shouldPushStackElementBetween`,
//! `getTypingOperation` in `src/vs/editor/common/cursor/cursorTypeEditOperations.ts`)
//! and reimplemented here; see `ATTRIBUTION.md`.

/// What kind of edit produced a change; decides where undo steps begin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// Programmatic or structural edits: always their own undo step.
    Other,
    /// Backspace.
    DeletingLeft,
    /// Delete.
    DeletingRight,
    /// Typing anything but a space.
    TypingOther,
    /// Typing a space after something that is not a space.
    TypingFirstSpace,
    /// Typing a space after a space.
    TypingConsecutiveSpace,
}

impl EditKind {
    /// The kind of typing `text`, given the previous edit kind.
    #[must_use]
    pub fn of_typing(text: &str, previous: Self) -> Self {
        match (text, previous) {
            (" ", Self::TypingFirstSpace | Self::TypingConsecutiveSpace) => {
                Self::TypingConsecutiveSpace
            }
            (" ", _) => Self::TypingFirstSpace,
            _ => Self::TypingOther,
        }
    }

    fn is_typing(self) -> bool {
        matches!(
            self,
            Self::TypingOther | Self::TypingFirstSpace | Self::TypingConsecutiveSpace
        )
    }

    fn normalized(self) -> Self {
        match self {
            Self::TypingConsecutiveSpace => Self::TypingFirstSpace,
            other => other,
        }
    }
}

/// Whether an undo step boundary goes between two consecutive edits.
fn starts_new_step(previous: EditKind, next: EditKind) -> bool {
    if previous.is_typing() && !next.is_typing() {
        return true;
    }
    if previous == EditKind::TypingFirstSpace {
        // `abc |d` stays one step; `abc  |d` does not.
        return false;
    }
    previous.normalized() != next.normalized()
}

/// One replacement: at `offset`, `removed` was replaced by `inserted`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub offset: usize,
    pub removed: String,
    pub inserted: String,
}

/// One undo step: its changes in order, and the cursor and version around it.
#[derive(Debug, Clone)]
pub(crate) struct Step {
    pub changes: Vec<Change>,
    pub cursor_before: usize,
    pub cursor_after: usize,
    pub version_before: u64,
    pub version_after: u64,
}

/// The undo and redo stacks.
#[derive(Debug, Clone)]
pub(crate) struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    previous: EditKind,
    open: bool,
}

impl Default for History {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            previous: EditKind::Other,
            open: false,
        }
    }
}

impl History {
    /// The kind of the last recorded edit.
    pub fn previous(&self) -> EditKind {
        self.previous
    }

    /// Ends the current undo step, e.g. when the cursor moves.
    pub fn close_step(&mut self) {
        self.reset();
    }

    /// Records a change made by an edit of `kind`.
    pub fn record(
        &mut self,
        kind: EditKind,
        change: Change,
        cursors: (usize, usize),
        versions: (u64, u64),
    ) {
        self.redo.clear();
        let extend = self.open && kind != EditKind::Other && !starts_new_step(self.previous, kind);
        match self.undo.last_mut() {
            Some(step) if extend => {
                step.changes.push(change);
                step.cursor_after = cursors.1;
                step.version_after = versions.1;
            }
            _ => self.undo.push(Step {
                changes: vec![change],
                cursor_before: cursors.0,
                cursor_after: cursors.1,
                version_before: versions.0,
                version_after: versions.1,
            }),
        }
        self.previous = kind;
        self.open = kind != EditKind::Other;
    }

    /// Takes the step to undo, moving it to the redo stack.
    pub fn undo(&mut self) -> Option<Step> {
        let step = self.undo.pop()?;
        self.redo.push(step.clone());
        self.reset();
        Some(step)
    }

    /// Takes the step to redo, moving it back to the undo stack.
    pub fn redo(&mut self) -> Option<Step> {
        let step = self.redo.pop()?;
        self.undo.push(step.clone());
        self.reset();
        Some(step)
    }

    fn reset(&mut self) {
        self.previous = EditKind::Other;
        self.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use EditKind::{
        DeletingLeft, DeletingRight, Other, TypingConsecutiveSpace, TypingFirstSpace, TypingOther,
    };

    #[test]
    fn grouping_rule_matches_vs_code() {
        assert!(!starts_new_step(TypingOther, TypingOther));
        assert!(starts_new_step(TypingOther, TypingFirstSpace));
        assert!(!starts_new_step(TypingFirstSpace, TypingOther));
        assert!(!starts_new_step(TypingFirstSpace, TypingConsecutiveSpace));
        assert!(!starts_new_step(
            TypingConsecutiveSpace,
            TypingConsecutiveSpace
        ));
        assert!(starts_new_step(TypingConsecutiveSpace, TypingOther));
        assert!(starts_new_step(TypingOther, DeletingLeft));
        assert!(!starts_new_step(DeletingLeft, DeletingLeft));
        assert!(starts_new_step(DeletingLeft, DeletingRight));
        assert!(starts_new_step(DeletingLeft, TypingOther));
        assert!(starts_new_step(Other, TypingOther));
    }

    #[test]
    fn typing_kind_depends_on_the_previous_space() {
        assert_eq!(EditKind::of_typing(" ", TypingOther), TypingFirstSpace);
        assert_eq!(
            EditKind::of_typing(" ", TypingFirstSpace),
            TypingConsecutiveSpace
        );
        assert_eq!(EditKind::of_typing("a", TypingFirstSpace), TypingOther);
    }
}
