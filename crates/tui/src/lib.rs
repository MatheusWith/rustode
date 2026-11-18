//! Terminal frontend (ratatui).
//!
//! [`App`] holds a workspace with one open document, turns terminal events
//! into workspace commands and draws only from workspace state. The layout
//! follows the spec's width bands: sidebar, editor and status bar at 120
//! columns or more; no sidebar below 120; editor only below 60; a notice
//! below 40×10.

mod app;
mod extensions;
mod find;
mod keys;
mod problems;
mod theme;
mod view;

pub use app::App;
pub use theme::ColorMode;
