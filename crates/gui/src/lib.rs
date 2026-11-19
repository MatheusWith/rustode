//! Graphical frontend: an egui window on eframe with wgpu.
//!
//! - **Reactive repaint only.** eframe draws a frame when input arrives or
//!   when a frame asks for another one, and nothing here asks: the caret does
//!   not blink and nothing animates, so an idle window draws nothing.
//! - **Virtualized editor.** The editor widget asks the workspace only for
//!   the lines in view, one by one, never for the whole text.
//! - **Workbench.** The explorer in a sidebar, a tab strip over the
//!   editor and the status bar, all drawn from the workspace's state.
//! - **AccessKit on.** The editor is a multi-line text input named after the
//!   file; its text runs are the visible lines and its text selection is the
//!   primary selection. The tab strip is a tab list, the explorer a tree of
//!   tree items with their level and expanded state, and the status bar a
//!   status node of labels.
//! - **Input-to-present latency** is timed by [`run`]; see [`latency`].
//!
//! - **Extensions view** in the sidebar on Ctrl+Shift+X: Open VSX search, install,
//!   uninstall and the installed list, all in the background; nothing is
//!   loaded (`extensions.rs`).
//! - **Diagnostics** from language servers are squiggled in the editor,
//!   counted in the status bar and listed in the problems panel
//!   (Ctrl+Shift+M); see `problems.rs`.
//! - **Search view** in the sidebar on Ctrl+Shift+F: a search across the
//!   folder whose results stream into a list (`search.rs`).
//! - **Width and height breakpoints.** The sidebar follows the window's
//!   width as VS Code's grid does and collapses below 390 points; the
//!   status bar drops items that no longer fit, and goes when the window is
//!   too short (`layout.rs`, `status.rs`).
//! - **Any scale factor.** Sizes are in points at the platform's scale
//!   factor, and the editor's lines, selections and caret sit on whole
//!   pixels (`editor.rs`).
//! - **Keyboard only.** F6 and Shift+F6 cycle the parts, Tab and Shift+Tab
//!   move through them, each focused element is outlined, and the AccessKit
//!   focus follows (`focus.rs`).
//! - **Quick input and find.** Ctrl+P, Ctrl+Shift+P or F1, and Ctrl+G open
//!   the quick input over the editor area: an editable combo box over a list
//!   box of files, commands from the workspace's registry, or a line
//!   (`quick_input.rs`). Ctrl+F and Ctrl+H open the find widget at the
//!   editor's top right, with every match in view highlighted (`find.rs`).
//! - **Change Language Mode** from a click on the status bar's language or
//!   Ctrl+K M: a list picker over the editor (`language.rs`).
//!
//! The GUI holds no editing logic: it maps input to workspace commands and
//! draws workspace state.

mod a11y;
mod app;
mod editor;
mod explorer;
mod extensions;
mod find;
mod focus;
pub mod fonts;
mod input;
mod keys;
mod language;
pub mod latency;
pub mod layout;
mod problems;
mod quick_input;
mod run;
mod search;
mod status;
mod tabs;
mod text;

pub use app::EditorApp;
pub use editor::{FONT_SIZE, LINE_HEIGHT, line_height};
pub use explorer::{HEADER_HEIGHT, ROW_HEIGHT, TITLE_HEIGHT};
pub use problems::PANEL_HEIGHT;
pub use run::{native_options, run, run_timed};
pub use status::STATUS_BAR_HEIGHT;
pub use tabs::TAB_HEIGHT;
