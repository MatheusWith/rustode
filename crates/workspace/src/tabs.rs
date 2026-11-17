//! Editor groups and their tabs.
//!
//! Phase 1 has one group; splits (ticket 35) add more. As in VS Code, a file
//! has one tab per group, next/previous tab wrap around
//! (`workbench.action.nextEditor`, `src/vs/workbench/browser/parts/editor/editorActions.ts:1265-1330`),
//! and closing a tab activates the most recently used one
//! (`workbench.editor.focusRecentEditorAfterClose`, default true,
//! `src/vs/workbench/browser/workbench.contribution.ts:321-325`).

use crate::DocumentId;

/// The tabs of one editor group, in display order, with the active one.
#[derive(Debug, Clone, Default)]
pub struct EditorGroup {
    tabs: Vec<DocumentId>,
    /// Most recently used first.
    recent: Vec<DocumentId>,
}

impl EditorGroup {
    /// The tabs, in display order.
    #[must_use]
    pub fn tabs(&self) -> &[DocumentId] {
        &self.tabs
    }

    /// The active tab.
    #[must_use]
    pub fn active(&self) -> Option<DocumentId> {
        self.recent.first().copied()
    }

    /// Adds `id` at the end if it has no tab yet, and activates it.
    pub(crate) fn open(&mut self, id: DocumentId) {
        if !self.tabs.contains(&id) {
            self.tabs.push(id);
        }
        self.activate(id);
    }

    /// Activates `id`; returns whether it has a tab.
    pub(crate) fn activate(&mut self, id: DocumentId) -> bool {
        if !self.tabs.contains(&id) {
            return false;
        }
        self.recent.retain(|&r| r != id);
        self.recent.insert(0, id);
        true
    }

    /// Activates the tab `step` places away from the active one, wrapping.
    pub(crate) fn cycle(&mut self, forward: bool) {
        let Some(active) = self.active() else { return };
        let Some(index) = self.tabs.iter().position(|&t| t == active) else {
            return;
        };
        let count = self.tabs.len();
        let next = if forward {
            (index + 1) % count
        } else {
            (index + count - 1) % count
        };
        self.activate(self.tabs[next]);
    }

    /// Removes `id`'s tab; the most recently used tab becomes active.
    pub(crate) fn close(&mut self, id: DocumentId) -> bool {
        let had = self.tabs.contains(&id);
        self.tabs.retain(|&t| t != id);
        self.recent.retain(|&r| r != id);
        had
    }
}
