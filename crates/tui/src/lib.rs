//! Terminal frontend (ratatui).
//!
//! [`App`] holds a workspace with one open document, turns terminal events
//! into workspace commands and draws only from workspace state. The layout
//! follows the spec's width bands: sidebar, editor and status bar at 120
//! columns or more; no sidebar below 120; editor only below 60; a notice
//! below 40×10. Editor groups split the editor area (see `groups`); a split
//! that does not fit shows only the group that has the focus.

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::unimplemented,
        clippy::todo
    )
)]

mod app;
mod breakpoint;
mod compare;
mod conflict;
mod debug;
mod extensions;
mod find;
mod groups;
mod keys;
mod language;
mod navigate;
mod problems;
mod search;
mod suggest;
mod theme;
mod view;

pub use app::App;
pub use theme::ColorMode;
