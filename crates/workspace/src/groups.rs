//! Editor groups: the layout of the editor area, and what a split does.
//!
//! Studied in VS Code and reimplemented (see `ATTRIBUTION.md`, ADR 0031):
//!
//! - the groups sit in a tree of splits, each split either side by side or
//!   stacked, and a group added next to one whose parent splits the same
//!   way joins that parent (`SerializableGrid`, `src/vs/base/browser/ui/grid/grid.ts`;
//!   `EditorPart.addGroup`, `src/vs/workbench/browser/parts/editor/editorPart.ts:674-700`);
//! - `workbench.action.splitEditor*` copies the active editor into a new
//!   group on the right or below and focuses it
//!   (`splitEditor`, `src/vs/workbench/browser/parts/editor/editorCommands.ts:752-772`);
//! - `workbench.action.focus<Left,Right,Above,Below>Group` finds the
//!   neighbor in the grid and wraps around
//!   (`AbstractFocusGroupAction`, `src/vs/workbench/browser/parts/editor/editorActions.ts:280-292`;
//!   `doFindGroupByDirection`, `editorPart.ts:405-413`);
//! - Ctrl+1, Ctrl+2, … focus the nth group in grid order, and Ctrl+N creates
//!   a group when N is one past the last
//!   (`registerFocusEditorGroupAtIndexCommands`, `editorCommands.ts:694-718`);
//! - a group left with no editor closes, unless it is the last one
//!   (`workbench.editor.closeEmptyGroups`, `editorGroupView.ts:2053-2055`);
//! - removing a group that still has editors merges them into the most
//!   recently active other group (`doRemoveGroupWithEditors`, `editorPart.ts:894-907`),
//!   and the group that becomes active is the most recently active one
//!   (`doRemoveEmptyGroup`, `editorPart.ts:909-917`);
//! - "Join All Editor Groups" merges every group into the active one
//!   (`mergeAllGroups`, `editorPart.ts:1092-1112`).
//!
//! A document is one text with one undo history, however many groups show
//! it; each group has its own cursors and selections for it. The document
//! holds the selections of the active group. A group that loses focus keeps
//! its own in its [`EditorGroup`], and every edit moves them
//! ([`rustode_core::CharEdit`]), so a cursor stays on the same text when
//! another view types above it.

use crate::{DocumentId, EditorGroup, Error, Event, Value, Workspace};
use rustode_core::{CharEdit, Selection, Selections};
use std::collections::HashSet;

/// Identifies an editor group. Ids are never reused within a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(usize);

/// How a split lays out its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Side by side: left to right.
    Horizontal,
    /// Stacked: top to bottom.
    Vertical,
}

/// Where a split puts the new group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    /// To the right of the group.
    Right,
    /// Below the group.
    Down,
}

/// A direction to move the focus in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// To the left.
    Left,
    /// To the right.
    Right,
    /// Up.
    Up,
    /// Down.
    Down,
}

/// How the editor area is divided: a tree whose leaves are groups. Sizes
/// are equal; a split's children share its space evenly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    /// One group.
    Group(GroupId),
    /// Two or more children laid out along `axis`.
    Split {
        /// Whether the children sit side by side or stacked.
        axis: Axis,
        /// The children, left to right or top to bottom; at least two.
        children: Vec<Layout>,
    },
}

impl Layout {
    /// The groups, left to right and top to bottom: VS Code's grid order.
    #[must_use]
    pub fn groups(&self) -> Vec<GroupId> {
        match self {
            Self::Group(id) => vec![*id],
            Self::Split { children, .. } => children.iter().flat_map(Self::groups).collect(),
        }
    }

    /// Puts `new` after `target`, splitting along `axis`.
    fn split(&mut self, target: GroupId, axis: Axis, new: GroupId) -> bool {
        match self {
            Self::Group(id) if *id == target => {
                *self = Self::Split {
                    axis,
                    children: vec![Self::Group(target), Self::Group(new)],
                };
                true
            }
            Self::Group(_) => false,
            Self::Split {
                axis: own,
                children,
            } => {
                let at = children
                    .iter()
                    .position(|c| matches!(c, Self::Group(id) if *id == target));
                if let Some(at) = at
                    && *own == axis
                {
                    children.insert(at + 1, Self::Group(new));
                    return true;
                }
                children.iter_mut().any(|c| c.split(target, axis, new))
            }
        }
    }

    /// The layout without `gone`; a split left with one child is that
    /// child, and a child split along its parent's axis joins the parent.
    fn without(self, gone: GroupId) -> Option<Self> {
        match self {
            Self::Group(id) => (id != gone).then_some(Self::Group(id)),
            Self::Split { axis, children } => {
                let mut flat = Vec::new();
                for child in children.into_iter().filter_map(|c| c.without(gone)) {
                    match child {
                        Self::Split {
                            axis: inner,
                            children,
                        } if inner == axis => flat.extend(children),
                        other => flat.push(other),
                    }
                }
                if flat.len() > 1 {
                    Some(Self::Split {
                        axis,
                        children: flat,
                    })
                } else {
                    flat.pop()
                }
            }
        }
    }

    /// The child indexes from this node down to `id`.
    fn path_to(&self, id: GroupId) -> Option<Vec<usize>> {
        match self {
            Self::Group(own) => (*own == id).then(Vec::new),
            Self::Split { children, .. } => children.iter().enumerate().find_map(|(i, c)| {
                c.path_to(id).map(|mut rest| {
                    rest.insert(0, i);
                    rest
                })
            }),
        }
    }

    fn node_at(&self, path: &[usize]) -> Option<&Self> {
        match (path.split_first(), self) {
            (None, node) => Some(node),
            (Some((&i, rest)), Self::Split { children, .. }) => children.get(i)?.node_at(rest),
            _ => None,
        }
    }

    /// The group reached by entering this node from its near side when
    /// moving along `axis` (`forward`: from the left or the top).
    fn entry(&self, axis: Axis, forward: bool) -> Option<GroupId> {
        match self {
            Self::Group(id) => Some(*id),
            Self::Split {
                axis: own,
                children,
            } => {
                let child = if *own == axis && !forward {
                    children.last()
                } else {
                    children.first()
                };
                child?.entry(axis, forward)
            }
        }
    }

    /// The group next to `from` in `direction`; with `wrap`, past the edge
    /// it comes round to the other side.
    fn neighbor(&self, from: GroupId, direction: Direction, wrap: bool) -> Option<GroupId> {
        let (axis, forward) = match direction {
            Direction::Left => (Axis::Horizontal, false),
            Direction::Right => (Axis::Horizontal, true),
            Direction::Up => (Axis::Vertical, false),
            Direction::Down => (Axis::Vertical, true),
        };
        let path = self.path_to(from)?;
        for depth in (0..path.len()).rev() {
            if let Some(Self::Split {
                axis: own,
                children,
            }) = self.node_at(&path[..depth])
                && *own == axis
            {
                let at = path[depth];
                let next = if forward {
                    at.checked_add(1)
                } else {
                    at.checked_sub(1)
                };
                if let Some(sibling) = next.and_then(|n| children.get(n)) {
                    return sibling.entry(axis, forward);
                }
            }
        }
        if !wrap {
            return None;
        }
        (0..path.len()).find_map(|depth| match self.node_at(&path[..depth]) {
            Some(Self::Split {
                axis: own,
                children,
            }) if *own == axis => {
                let edge = if forward {
                    children.first()
                } else {
                    children.last()
                };
                edge?.entry(axis, forward)
            }
            _ => None,
        })
    }
}

/// Every group, their layout and which one has focus.
#[derive(Debug)]
pub(crate) struct Groups {
    list: Vec<(GroupId, EditorGroup)>,
    pub(crate) layout: Layout,
    pub(crate) active: GroupId,
    /// Most recently active first.
    recent: Vec<GroupId>,
    next: usize,
}

impl Default for Groups {
    fn default() -> Self {
        let first = GroupId(0);
        Self {
            list: vec![(first, EditorGroup::default())],
            layout: Layout::Group(first),
            active: first,
            recent: vec![first],
            next: 1,
        }
    }
}

impl Groups {
    pub(crate) fn get(&self, id: GroupId) -> Option<&EditorGroup> {
        self.list.iter().find(|(g, _)| *g == id).map(|(_, e)| e)
    }

    pub(crate) fn get_mut(&mut self, id: GroupId) -> Option<&mut EditorGroup> {
        self.list.iter_mut().find(|(g, _)| *g == id).map(|(_, e)| e)
    }

    /// The active group; the layout always has one.
    pub(crate) fn active_group(&self) -> &EditorGroup {
        static EMPTY: std::sync::LazyLock<EditorGroup> =
            std::sync::LazyLock::new(EditorGroup::default);
        self.get(self.active).unwrap_or(&EMPTY)
    }

    pub(crate) fn active_group_mut(&mut self) -> &mut EditorGroup {
        let active = self.active;
        if self.get(active).is_none() {
            self.list.push((active, EditorGroup::default()));
        }
        let at = self
            .list
            .iter()
            .position(|(g, _)| *g == active)
            .unwrap_or(0);
        &mut self.list[at].1
    }

    pub(crate) fn all(&mut self) -> impl Iterator<Item = (GroupId, &mut EditorGroup)> {
        self.list.iter_mut().map(|(g, e)| (*g, e))
    }

    /// The number of groups showing `id`.
    pub(crate) fn showing(&self, id: DocumentId) -> usize {
        self.list
            .iter()
            .filter(|(_, g)| g.tabs().contains(&id))
            .count()
    }

    fn create(&mut self, next_to: GroupId, axis: Axis) -> GroupId {
        let id = GroupId(self.next);
        self.next += 1;
        self.list.push((id, EditorGroup::default()));
        if !self.layout.split(next_to, axis, id) {
            // `next_to` is not in the layout: put the group beside the first.
            let first = self.layout.groups().first().copied().unwrap_or(next_to);
            self.layout.split(first, axis, id);
        }
        self.recent.push(id);
        id
    }

    fn activate(&mut self, id: GroupId) {
        self.active = id;
        self.recent.retain(|&g| g != id);
        self.recent.insert(0, id);
    }

    /// Takes `id` out of the layout. The caller makes another group active
    /// first.
    fn remove(&mut self, id: GroupId) {
        self.list.retain(|(g, _)| *g != id);
        self.recent.retain(|&g| g != id);
        let layout = std::mem::replace(&mut self.layout, Layout::Group(id));
        self.layout = layout.without(id).unwrap_or(Layout::Group(self.active));
    }

    /// The most recently active group that is not `except`.
    fn most_recent_other(&self, except: GroupId) -> Option<GroupId> {
        self.recent.iter().copied().find(|&g| g != except)
    }
}

impl Workspace {
    /// How the editor area is divided into groups.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.groups.layout
    }

    /// The groups in grid order: left to right, top to bottom.
    #[must_use]
    pub fn group_ids(&self) -> Vec<GroupId> {
        self.groups.layout.groups()
    }

    /// The group with focus.
    #[must_use]
    pub fn active_group(&self) -> GroupId {
        self.groups.active
    }

    /// A group by id.
    #[must_use]
    pub fn group_by_id(&self, id: GroupId) -> Option<&EditorGroup> {
        self.groups.get(id)
    }

    /// The cursors and selections one group has in `document`: the document's
    /// own for the active group, the group's kept ones otherwise. `None`
    /// when the group does not show the document. The offsets are clamped
    /// to the text.
    #[must_use]
    pub fn view_selections(&self, group: GroupId, document: DocumentId) -> Option<Selections> {
        let shown = self.groups.get(group)?;
        if !shown.tabs().contains(&document) {
            return None;
        }
        let doc = self.document(document)?;
        let own = if group == self.groups.active {
            doc.editor().selection_set()
        } else {
            shown
                .views
                .get(&document)
                .cloned()
                .unwrap_or_else(|| doc.editor().selection_set())
        };
        let len = doc.len_chars();
        Some(own.map(|o| o.min(len)))
    }

    /// The primary cursor of one group's view of `document`, as a char offset.
    #[must_use]
    pub fn view_cursor(&self, group: GroupId, document: DocumentId) -> Option<usize> {
        self.view_selections(group, document)
            .map(|s| s.primary().head)
    }

    /// Splits the active group: a new group opens to the right or below,
    /// shows the active editor too, and takes the focus. With nothing open
    /// the new group is empty.
    pub fn split_group(&mut self, direction: SplitDirection) -> GroupId {
        let axis = match direction {
            SplitDirection::Right => Axis::Horizontal,
            SplitDirection::Down => Axis::Vertical,
        };
        let from = self.groups.active;
        let shown = self.groups.get(from).and_then(EditorGroup::active);
        let new = self.groups.create(from, axis);
        self.focus_group_id(new);
        if let Some(id) = shown {
            self.open_in_group(new, id);
        }
        self.events.push(Event::GroupsChanged);
        new
    }

    /// `workbench.action.splitEditor` and `splitEditorRight`: the side the
    /// setting `workbench.editor.openSideBySideDirection` names (right unless
    /// it is `"down"`), whichever command it was for `splitEditor`.
    pub(crate) fn split_by_command(&mut self, id: &str) {
        let down = id == "workbench.action.splitEditor"
            && matches!(
                self.config.value("workbench.editor.openSideBySideDirection"),
                Some(Value::String(s)) if s == "down"
            );
        let direction = if down {
            SplitDirection::Down
        } else {
            SplitDirection::Right
        };
        self.split_group(direction);
    }

    /// Gives a group the focus.
    ///
    /// # Errors
    /// When no group has this id.
    pub fn focus_group(&mut self, group: GroupId) -> Result<(), Error> {
        if self.groups.get(group).is_none() {
            return Err(Error::UnknownGroup(group));
        }
        self.focus_group_id(group);
        Ok(())
    }

    /// Ctrl+1, Ctrl+2, …: focuses the group at `index` (0-based, in grid
    /// order). One past the last creates an empty group to its right, as
    /// VS Code does; further than that does nothing. Returns whether the
    /// focus moved.
    pub fn focus_group_index(&mut self, index: usize) -> bool {
        let ids = self.group_ids();
        if let Some(&group) = ids.get(index) {
            self.focus_group_id(group);
            return true;
        }
        if index == ids.len()
            && let Some(&last) = ids.last()
        {
            let new = self.groups.create(last, Axis::Horizontal);
            self.focus_group_id(new);
            self.events.push(Event::GroupsChanged);
            return true;
        }
        false
    }

    /// Focuses the group next to the active one, wrapping around. Returns
    /// whether the focus moved.
    pub fn focus_group_toward(&mut self, direction: Direction) -> bool {
        let from = self.groups.active;
        match self.groups.layout.neighbor(from, direction, true) {
            Some(to) if to != from => {
                self.focus_group_id(to);
                true
            }
            _ => false,
        }
    }

    /// The group `direction` of `group`, if there is one (no wrapping).
    #[must_use]
    pub fn group_toward(&self, group: GroupId, direction: Direction) -> Option<GroupId> {
        self.groups.layout.neighbor(group, direction, false)
    }

    /// Focuses the next group in grid order, wrapping.
    pub fn focus_next_group(&mut self, forward: bool) -> bool {
        let ids = self.group_ids();
        let Some(at) = ids.iter().position(|&g| g == self.groups.active) else {
            return false;
        };
        let to = if forward {
            ids[(at + 1) % ids.len()]
        } else {
            ids[(at + ids.len() - 1) % ids.len()]
        };
        let moved = to != self.groups.active;
        self.focus_group_id(to);
        moved
    }

    /// Closes one editor of a group. The document closes with it unless
    /// another group still shows it; unsaved changes are only dropped when
    /// `discard` is true. A group left empty closes, unless it is the last.
    ///
    /// # Errors
    /// When the group has no such tab, or the document is dirty, shown
    /// nowhere else and `discard` is false.
    pub fn close_editor(
        &mut self,
        group: GroupId,
        id: DocumentId,
        discard: bool,
    ) -> Result<(), Error> {
        let shown = self.groups.get(group).ok_or(Error::UnknownGroup(group))?;
        if !shown.tabs().contains(&id) {
            return Err(Error::UnknownDocument(id));
        }
        if self.groups.showing(id) > 1 {
            self.remove_tab(group, id);
        } else {
            self.close_document(id, discard)?;
        }
        self.remove_group_if_empty(group);
        Ok(())
    }

    /// Closes every editor of a group (Ctrl+K W) and the group with them
    /// unless it is the last. Nothing closes when an unsaved document shown
    /// only here would be lost and `discard` is false.
    ///
    /// # Errors
    /// When the group does not exist or a dirty document would be dropped.
    pub fn close_editors_in_group(&mut self, group: GroupId, discard: bool) -> Result<(), Error> {
        let tabs = self
            .groups
            .get(group)
            .ok_or(Error::UnknownGroup(group))?
            .tabs()
            .to_vec();
        if !discard
            && let Some(&dirty) = tabs.iter().find(|&&id| {
                self.groups.showing(id) == 1
                    && self.document(id).is_some_and(crate::Document::is_dirty)
            })
        {
            return Err(Error::Dirty(dirty));
        }
        for id in tabs {
            if self.groups.showing(id) > 1 {
                self.remove_tab(group, id);
            } else {
                self.close_document(id, true)?;
            }
        }
        self.remove_group_if_empty(group);
        Ok(())
    }

    /// Closes a group (`workbench.action.closeGroup`). Its editors move to
    /// the most recently active other group, so no document closes. Returns
    /// whether a group closed: the last group never does.
    ///
    /// # Errors
    /// When no group has this id.
    pub fn close_group(&mut self, group: GroupId) -> Result<bool, Error> {
        if self.groups.get(group).is_none() {
            return Err(Error::UnknownGroup(group));
        }
        let Some(target) = self.groups.most_recent_other(group) else {
            return Ok(false);
        };
        let tabs = self
            .groups
            .get(group)
            .map(|g| g.tabs().to_vec())
            .unwrap_or_default();
        if self.groups.active == group {
            self.focus_group_id(target);
        }
        for id in tabs {
            self.append_to_group(target, id);
        }
        self.drop_group(group);
        Ok(true)
    }

    /// Moves the active editor into the next or previous group in grid
    /// order (Ctrl+Alt+Right and Ctrl+Alt+Left), which takes the focus. A
    /// group left empty closes. Returns whether it moved.
    pub fn move_editor_to_group(&mut self, next: bool) -> bool {
        let ids = self.group_ids();
        let from = self.groups.active;
        let Some(at) = ids.iter().position(|&g| g == from) else {
            return false;
        };
        let to = if next {
            ids.get(at + 1)
        } else {
            at.checked_sub(1).and_then(|p| ids.get(p))
        };
        let (Some(&to), Some(id)) = (to, self.groups.get(from).and_then(EditorGroup::active))
        else {
            return false;
        };
        self.open_in_group(to, id);
        self.remove_tab(from, id);
        self.focus_group_id(to);
        self.remove_group_if_empty(from);
        self.events.push(Event::GroupsChanged);
        true
    }

    /// Joins every group into the active one (`workbench.action.joinAllGroups`):
    /// editors keep their order, group by group, most recently active
    /// first. Returns whether there was anything to join.
    pub fn join_all_groups(&mut self) -> bool {
        let active = self.groups.active;
        let others: Vec<GroupId> = self
            .groups
            .recent
            .iter()
            .copied()
            .filter(|&g| g != active)
            .collect();
        for group in &others {
            let tabs = self
                .groups
                .get(*group)
                .map(|g| g.tabs().to_vec())
                .unwrap_or_default();
            for id in tabs {
                self.append_to_group(active, id);
            }
            self.drop_group(*group);
        }
        !others.is_empty()
    }

    // ---- internals ----

    /// Puts `id` as the active tab of `group`.
    pub(crate) fn open_in_group(&mut self, group: GroupId, id: DocumentId) {
        if let Some(g) = self.groups.get_mut(group) {
            g.open(id);
        }
        self.share_changed(id);
    }

    fn append_to_group(&mut self, group: GroupId, id: DocumentId) {
        let seen: HashSet<DocumentId> = self
            .groups
            .get(group)
            .map(|g| g.tabs().iter().copied().collect())
            .unwrap_or_default();
        if !seen.contains(&id)
            && let Some(g) = self.groups.get_mut(group)
        {
            g.append(id);
        }
        self.share_changed(id);
    }

    fn remove_tab(&mut self, group: GroupId, id: DocumentId) {
        if let Some(g) = self.groups.get_mut(group) {
            g.close(id);
        }
        self.share_changed(id);
    }

    /// Closes the document everywhere, as [`Workspace::close`] does.
    fn close_document(&mut self, id: DocumentId, discard: bool) -> Result<(), Error> {
        self.close(id, discard)
    }

    fn remove_group_if_empty(&mut self, group: GroupId) {
        let empty = self.groups.get(group).is_some_and(|g| g.tabs().is_empty());
        if empty && self.groups.list.len() > 1 {
            if self.groups.active == group
                && let Some(next) = self.groups.most_recent_other(group)
            {
                self.focus_group_id(next);
            }
            self.drop_group(group);
        }
    }

    /// Removes a group whose editors went elsewhere (or whose tabs are gone).
    fn drop_group(&mut self, group: GroupId) {
        let docs: Vec<DocumentId> = self
            .groups
            .get(group)
            .map(|g| g.tabs().to_vec())
            .unwrap_or_default();
        if self.groups.active == group
            && let Some(next) = self.groups.most_recent_other(group)
        {
            self.focus_group_id(next);
        }
        self.groups.remove(group);
        for id in docs {
            self.share_changed(id);
        }
        self.events.push(Event::GroupsChanged);
    }

    /// Closing a document removes its tabs from every group; groups left
    /// empty close.
    pub(crate) fn close_tabs_everywhere(&mut self, id: DocumentId) {
        let holders: Vec<GroupId> = self
            .groups
            .all()
            .filter(|(_, g)| g.tabs().contains(&id))
            .map(|(g, _)| g)
            .collect();
        for group in &holders {
            if let Some(g) = self.groups.get_mut(*group) {
                g.close(id);
            }
        }
        for group in holders {
            self.remove_group_if_empty(group);
        }
    }

    /// Moves the focus to `group`: the group being left keeps its own
    /// selections of documents another group shows, and the group
    /// entered gets its own back.
    pub(crate) fn focus_group_id(&mut self, group: GroupId) {
        let old = self.groups.active;
        if old == group {
            self.groups.activate(group);
            return;
        }
        self.sync_views_all();
        let shared = |ws: &Self, id: DocumentId| ws.groups.showing(id) > 1;
        let leaving: Vec<DocumentId> = self
            .groups
            .get(old)
            .map(|g| g.tabs().to_vec())
            .unwrap_or_default();
        for id in leaving {
            if shared(self, id)
                && let Some(set) = self.document(id).map(|d| d.editor().selection_set())
                && let Some(g) = self.groups.get_mut(old)
            {
                g.views.insert(id, set);
            }
        }
        self.groups.activate(group);
        let entering: Vec<(DocumentId, Selections)> = self
            .groups
            .get(group)
            .map(|g| g.views.iter().map(|(id, s)| (*id, s.clone())).collect())
            .unwrap_or_default();
        for (id, set) in entering {
            if let Ok(document) = self.document_mut(id) {
                document.editor_mut().set_selection_set(&set);
            }
        }
        self.events.push(Event::GroupsChanged);
    }

    /// After a document gained or lost a group: starts or stops following
    /// its edits, and gives a new view its starting selections.
    fn share_changed(&mut self, id: DocumentId) {
        let shared = self.groups.showing(id) > 1;
        let Some(set) = self.document(id).map(|d| d.editor().selection_set()) else {
            return;
        };
        if shared {
            for (_, group) in self.groups.all() {
                if group.tabs().contains(&id) {
                    group.views.entry(id).or_insert_with(|| set.clone());
                }
            }
        } else {
            for (_, group) in self.groups.all() {
                group.views.remove(&id);
            }
        }
        if let Ok(document) = self.document_mut(id) {
            if shared {
                document.editor_mut().record_char_edits();
            } else {
                document.editor_mut().stop_char_edits();
            }
        }
    }

    /// Moves every group's own selections of `id` through the edits made
    /// since the last call.
    pub(crate) fn sync_views(&mut self, id: DocumentId) {
        let Ok(document) = self.document_mut(id) else {
            return;
        };
        let edits = document.editor_mut().take_char_edits();
        if edits.is_empty() {
            return;
        }
        for (_, group) in self.groups.all() {
            if let Some(set) = group.views.get_mut(&id) {
                *set = follow(set, &edits);
            }
        }
    }

    pub(crate) fn sync_views_all(&mut self) {
        let shared: Vec<DocumentId> = (0..self.documents.len())
            .map(DocumentId)
            .filter(|&id| self.groups.showing(id) > 1)
            .collect();
        for id in shared {
            self.sync_views(id);
        }
    }
}

/// `set` after `edits`: a point follows the text it was on, a point inside
/// deleted text lands where the deletion was, and an empty cursor at the
/// place text is inserted moves after it, as typing in another view does.
fn follow(set: &Selections, edits: &[CharEdit]) -> Selections {
    let mut list: Vec<Selection> = set.as_slice().to_vec();
    for edit in edits {
        for selection in &mut list {
            *selection =
                if selection.is_empty() && edit.removed == 0 && selection.head == edit.start {
                    Selection::cursor(edit.start + edit.inserted)
                } else {
                    Selection::new(edit.map(selection.anchor), edit.map(selection.head))
                };
        }
    }
    Selections::from_list(list, set.primary_index())
}
