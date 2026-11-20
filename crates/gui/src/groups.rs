//! Where the editor groups go inside the editor area, and what the keys of
//! the group commands are.
//!
//! The workspace owns the layout (`Workspace::layout`, ADR 0031); this
//! module turns it into rectangles. Studied in VS Code (Code - OSS) and
//! reimplemented, see `ATTRIBUTION.md`: a group is never narrower than the
//! editor's 220-point minimum width nor shorter than a tab strip plus its
//! 70-point minimum height (`src/vs/workbench/browser/parts/editor/editor.ts:30`,
//! `editorPane.ts:65`), and VS Code's grid never lets a group go below
//! them (`src/vs/base/browser/ui/grid/gridview.ts`, `splitview.ts`). Where
//! a split cannot give every child that much, the window is too small
//! for it, and only the child that holds the active group is shown; the
//! others come back when the window grows (VS Code instead overflows the
//! grid and clips it). Children share a split equally, with a one-point
//! divider between neighbors: **the dividers do not drag yet** (the
//! workspace keeps no sizes, see `docs/gaps.md`).
//!
//! The keys are VS Code's defaults: Ctrl+\ splits, Ctrl+1 to Ctrl+3 focus the
//! first three groups, Ctrl+Alt+Right and Left move the editor, and after
//! Ctrl+K, Ctrl+arrow focuses the group in that direction and W closes the
//! group's editors (`src/vs/workbench/browser/parts/editor/editorActions.ts`,
//! `editorCommands.ts`).

use crate::layout::{EDITOR_MIN_HEIGHT, EDITOR_MIN_WIDTH as EDITOR_MIN_WIDTH_FOR_GROUPS};
use crate::tabs::TAB_HEIGHT;
use eframe::egui::{self, Context, Key, Pos2, Rect, pos2};
use rustode_workspace::{Axis, GroupId, Layout};

/// The gap between side-by-side groups, in points.
pub(crate) const DIVIDER: f32 = 1.0;

/// One group's place: its tab strip and its editor below it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    pub(crate) group: GroupId,
    pub(crate) tabs: Rect,
    pub(crate) editor: Rect,
}

/// The groups on screen and the dividers between them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Arranged {
    pub(crate) groups: Vec<Placed>,
    pub(crate) dividers: Vec<Rect>,
}

/// Places the groups of `layout` in `rect`; `active` is kept on screen when
/// a split does not fit.
pub(crate) fn arrange(layout: &Layout, rect: Rect, active: GroupId) -> Arranged {
    let mut out = Arranged::default();
    place(layout, rect, active, &mut out);
    out
}

fn place(layout: &Layout, rect: Rect, active: GroupId, out: &mut Arranged) {
    match layout {
        Layout::Group(group) => {
            let (tabs, editor) =
                rect.split_top_bottom_at_y(rect.top() + TAB_HEIGHT.min(rect.height()));
            out.groups.push(Placed {
                group: *group,
                tabs,
                editor,
            });
        }
        Layout::Split { axis, children } => {
            let count = f32::from(u16::try_from(children.len()).unwrap_or(u16::MAX));
            let gaps = (count - 1.0) * DIVIDER;
            let fits = match axis {
                Axis::Horizontal => rect.width() >= count * EDITOR_MIN_WIDTH_FOR_GROUPS + gaps,
                Axis::Vertical => rect.height() >= count * (TAB_HEIGHT + EDITOR_MIN_HEIGHT),
            };
            if !fits {
                if let Some(child) = children.iter().find(|c| c.groups().contains(&active)) {
                    place(child, rect, active, out);
                }
                return;
            }
            let along = match axis {
                Axis::Horizontal => rect.width(),
                Axis::Vertical => rect.height(),
            };
            let share = ((along - gaps) / count).floor();
            let mut at = match axis {
                Axis::Horizontal => rect.left(),
                Axis::Vertical => rect.top(),
            };
            for (i, child) in children.iter().enumerate() {
                let last = i + 1 == children.len();
                let child_rect = match axis {
                    Axis::Horizontal => {
                        let right = if last { rect.right() } else { at + share };
                        Rect::from_min_max(pos2(at, rect.top()), pos2(right, rect.bottom()))
                    }
                    Axis::Vertical => {
                        let bottom = if last { rect.bottom() } else { at + share };
                        Rect::from_min_max(pos2(rect.left(), at), pos2(rect.right(), bottom))
                    }
                };
                place(child, child_rect, active, out);
                if !last && *axis == Axis::Horizontal {
                    let x = child_rect.right();
                    out.dividers.push(Rect::from_min_max(
                        pos2(x, rect.top()),
                        Pos2::new(x + DIVIDER, rect.bottom()),
                    ));
                }
                at = match axis {
                    Axis::Horizontal => child_rect.right() + DIVIDER,
                    Axis::Vertical => child_rect.bottom(),
                };
            }
        }
    }
}

/// What the key that follows Ctrl+K asks of the groups.
#[derive(Debug, Default)]
pub(crate) struct Chord {
    pending: bool,
}

impl Chord {
    /// Takes the group commands out of this frame's input: Ctrl+K then
    /// Ctrl+arrow or W. The Ctrl+K itself stays for the other chords.
    pub(crate) fn take(&mut self, ctx: &Context) -> Vec<&'static str> {
        ctx.input_mut(|input| {
            let mut commands = Vec::new();
            let mut pending = self.pending;
            input.events.retain(|event| {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                else {
                    return true;
                };
                let ctrl = modifiers.ctrl || modifiers.command;
                if *key == Key::K && ctrl && !modifiers.alt && !modifiers.shift {
                    pending = true;
                    return true;
                }
                let second =
                    std::mem::take(&mut pending).then(|| second_key(*key, ctrl, modifiers.shift));
                if let Some(Some(id)) = second {
                    commands.push(id);
                    return false;
                }
                true
            });
            self.pending = pending;
            commands
        })
    }
}

fn second_key(key: Key, ctrl: bool, shift: bool) -> Option<&'static str> {
    if shift {
        return None;
    }
    match (key, ctrl) {
        (Key::ArrowLeft, true) => Some("workbench.action.focusLeftGroup"),
        (Key::ArrowRight, true) => Some("workbench.action.focusRightGroup"),
        (Key::ArrowUp, true) => Some("workbench.action.focusAboveGroup"),
        (Key::ArrowDown, true) => Some("workbench.action.focusBelowGroup"),
        (Key::W, _) => Some("workbench.action.closeEditorsInGroup"),
        _ => None,
    }
}

/// The group command a single chord runs.
pub(crate) fn key_command(key: Key, ctrl: bool, shift: bool, alt: bool) -> Option<&'static str> {
    if !ctrl || shift {
        return None;
    }
    match (key, alt) {
        (Key::Backslash, false) => Some("workbench.action.splitEditor"),
        (Key::Num1, false) => Some("workbench.action.focusFirstEditorGroup"),
        (Key::Num2, false) => Some("workbench.action.focusSecondEditorGroup"),
        (Key::Num3, false) => Some("workbench.action.focusThirdEditorGroup"),
        (Key::ArrowRight, true) => Some("workbench.action.moveEditorToNextGroup"),
        (Key::ArrowLeft, true) => Some("workbench.action.moveEditorToPreviousGroup"),
        _ => None,
    }
}

/// Whether `id` is one of the editor group commands.
pub(crate) fn is_group_command(id: &str) -> bool {
    id.contains("Group") || id.starts_with("workbench.action.splitEditor")
}
