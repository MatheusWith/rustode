//! `rustode-tui PATH...`: the terminal editor. Files open in tabs; the
//! first folder opens in the explorer.
//!
//! Logs go to stderr (`RUSTODE_LOG`, `RUSTODE_LOG_FORMAT`); redirect it to a
//! file when logging, since the editor owns the terminal. On exit the
//! key-to-frame latency is logged at `info`.

use ratatui::crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use ratatui::crossterm::execute;
use rustode_tui::App;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(err) = rustode_workspace::trace::init_from_env() {
        eprintln!("warning: {err}");
    }
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        eprintln!("Usage: rustode-tui PATH...");
        return ExitCode::from(2);
    }
    let mut app = match App::open_paths(&paths) {
        Ok(app) => app,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste);
    let result = app.run(&mut terminal);
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    if let Some(report) = app.latency() {
        tracing::info!(%report, "key-to-frame latency");
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
