//! Where the parts go as the window changes size: the sidebar on the left,
//! the tab strip and the editor on the right, the status bar along the
//! bottom.
//!
//! Studied in VS Code (Code - OSS) and reimplemented, see `ATTRIBUTION.md`.
//! VS Code lays its parts out in a grid that keeps the sidebar's width and
//! gives every change of the window's width to the editor first
//! (`proportionalLayout: false`, `src/vs/workbench/browser/layout.ts:1706`;
//! the editor's `LayoutPriority.High` and the sidebar's `Low`,
//! `src/vs/workbench/browser/parts/editor/editorPart.ts:1189`,
//! `src/vs/workbench/browser/parts/sidebar/sidebarPart.ts:56`, applied by
//! `src/vs/base/browser/ui/splitview/splitview.ts:853-864`, `:1257-1340`):
//!
//! - the sidebar starts `min(300, width / 4)` points wide
//!   (`layout.ts:2908`, `:3055`) and never narrower than 170
//!   (`sidebarPart.ts:50`);
//! - a narrowing window shrinks the editor down to its 220-point minimum
//!   (`src/vs/workbench/browser/parts/editor/editor.ts:30`,
//!   `editorPane.ts:65`), then the sidebar down to its own; a widening
//!   window widens the editor only, so a sidebar that shrank stays narrow.
//!
//! Past both minimums VS Code lets its grid overflow and clips the editor.
//! rustode collapses the sidebar there instead, below 390 points (170 +
//! 220; rustode has no 48-point activity bar), so the editor never drops
//! under its minimum on its own. It comes back when the window widens past
//! 390 again. Asking for the sidebar while it is collapsed (Ctrl+B,
//! Ctrl+Shift+E, Ctrl+Shift+X) shows it anyway, at most half the window
//! wide, until the window next crosses 390.
//!
//! In height, the status bar goes first when the window is too short for
//! the tab strip, the editor's 70-point minimum height (`editor.ts:30`) and
//! the status bar: the bottom of VS Code's grid is what its overflow cuts.
//!
//! The window itself starts at VS Code's minimum size, 400 by 270 points
//! (`src/vs/platform/window/common/window.ts:23-27`, applied in
//! `src/vs/platform/windows/electron-main/windows.ts:192-193`); see
//! `run.rs`.
//!
//! All sizes are in points: at any scale factor the parts keep the same
//! size in points, and egui draws them in that many physical pixels.

use crate::status::STATUS_BAR_HEIGHT;
use crate::tabs::TAB_HEIGHT;
use eframe::egui::Rect;

/// The sidebar's minimum width, in points.
pub const SIDEBAR_MIN_WIDTH: f32 = 170.0;
/// The editor's minimum width, in points.
pub const EDITOR_MIN_WIDTH: f32 = 220.0;
/// The editor's minimum height, in points.
pub const EDITOR_MIN_HEIGHT: f32 = 70.0;
/// Windows narrower than this collapse the sidebar.
pub const SIDEBAR_BREAKPOINT: f32 = SIDEBAR_MIN_WIDTH + EDITOR_MIN_WIDTH;
/// Windows shorter than this hide the status bar.
pub const STATUS_BAR_BREAKPOINT: f32 = TAB_HEIGHT + EDITOR_MIN_HEIGHT + STATUS_BAR_HEIGHT;
/// The widest the sidebar starts.
const SIDEBAR_START_WIDTH: f32 = 300.0;
/// VS Code's minimum window size, in points.
pub(crate) const WINDOW_MIN_SIZE: [f32; 2] = [400.0, 270.0];

/// The sidebar's place in the layout, kept between frames.
#[derive(Debug)]
pub(crate) struct Sidebar {
    /// Whether the user wants it (Ctrl+B).
    shown: bool,
    /// Its width while shown, set on the first frame.
    width: Option<f32>,
    /// Whether a narrow window collapsed it.
    collapsed: bool,
    /// The window's width on the last frame.
    window: Option<f32>,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self {
            shown: true,
            width: None,
            collapsed: false,
            window: None,
        }
    }
}

impl Sidebar {
    /// Whether the sidebar is on screen.
    pub(crate) fn visible(&self) -> bool {
        self.shown && !self.collapsed
    }

    /// Ctrl+B: hides a visible sidebar, or shows a hidden or collapsed one.
    pub(crate) fn toggle(&mut self) {
        if self.visible() {
            self.shown = false;
        } else {
            self.reveal();
        }
    }

    /// Shows the sidebar, even when the window is narrow enough to
    /// collapse it.
    pub(crate) fn reveal(&mut self) {
        self.shown = true;
        self.collapsed = false;
    }

    /// The sidebar's width in a window `window` points wide, or `None`
    /// while it is not shown. Call it once a frame: it follows resizes.
    pub(crate) fn fit(&mut self, window: f32) -> Option<f32> {
        let narrow = window < SIDEBAR_BREAKPOINT;
        let was_narrow = self.window.is_some_and(|last| last < SIDEBAR_BREAKPOINT);
        if !narrow {
            self.collapsed = false;
        } else if !was_narrow {
            // Crossing the breakpoint, or starting below it, collapses it.
            self.collapsed = true;
        }
        self.window = Some(window);
        let mut width = self
            .width
            .unwrap_or_else(|| (window / 4.0).clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_START_WIDTH));
        if self.shown {
            // The editor gives up its width first, down to its minimum.
            width = width.min(window - EDITOR_MIN_WIDTH).max(SIDEBAR_MIN_WIDTH);
        }
        self.width = Some(width);
        if !self.visible() {
            return None;
        }
        Some(if narrow {
            width.min(window / 2.0)
        } else {
            width
        })
    }
}

/// Where each part goes this frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Areas {
    pub(crate) sidebar: Option<Rect>,
    pub(crate) tabs: Rect,
    pub(crate) editor: Rect,
    pub(crate) status: Option<Rect>,
}

/// The parts in `rect`, the sidebar `sidebar` points wide if shown.
pub(crate) fn areas(rect: Rect, sidebar: Option<f32>) -> Areas {
    let (body, status) = if rect.height() < STATUS_BAR_BREAKPOINT {
        (rect, None)
    } else {
        let (body, status) = rect.split_top_bottom_at_y(rect.bottom() - STATUS_BAR_HEIGHT);
        (body, Some(status))
    };
    let (sidebar, group) = match sidebar {
        Some(width) => {
            let (sidebar, group) = body.split_left_right_at_x(body.left() + width);
            (Some(sidebar), group)
        }
        None => (None, body),
    };
    let (tabs, editor) = group.split_top_bottom_at_y(group.top() + TAB_HEIGHT.min(group.height()));
    Areas {
        sidebar,
        tabs,
        editor,
        status,
    }
}
