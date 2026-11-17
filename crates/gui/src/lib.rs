//! Graphical frontend: an egui window on eframe with wgpu.
//!
//! - **Reactive repaint only.** eframe draws a frame when input arrives or
//!   when a frame asks for another one, and nothing here asks: the caret does
//!   not blink and nothing animates, so an idle window draws nothing.
//! - **Virtualized editor.** The editor widget asks the workspace only for
//!   the lines in view, one by one, never for the whole text.
//! - **AccessKit on.** The editor is a multi-line text input named after the
//!   file; its text runs are the visible lines and its text selection is the
//!   primary selection. The status bar items are labels.
//! - **Input-to-present latency** is timed by [`run`]; see [`latency`].
//!
//! The GUI holds no editing logic: it maps input to workspace commands and
//! draws workspace state.

mod app;
mod editor;
pub mod fonts;
mod keys;
pub mod latency;
mod run;
mod status;
mod text;

pub use app::EditorApp;
pub use editor::{FONT_SIZE, LINE_HEIGHT};
pub use run::{native_options, run, run_timed};
pub use status::STATUS_BAR_HEIGHT;
