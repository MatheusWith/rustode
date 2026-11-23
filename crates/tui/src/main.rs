//! `rustode-tui PATH...`: the terminal editor. Files open in tabs; the
//! first folder opens in the explorer.
//!
//! Logs go to stderr (`RUSTODE_LOG`, `RUSTODE_LOG_FORMAT`); redirect it to a
//! file when logging, since the editor owns the terminal. On exit the
//! key-to-frame latency is logged at `info`.
//!
//! Extensions live in rustode's own folder (`RUSTODE_EXTENSIONS_DIR`, else
//! `rustode/extensions` under the XDG data folder), and which are disabled
//! in `globalStorage/state.json` in the user folder.

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

use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
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
    app.load_user_config(&user_dir());
    // The user's own settings are loaded, so a built-in server starts only
    // for a language they leave alone.
    app.workspace_mut()
        .use_default_language_servers(&std::env::var_os("PATH").unwrap_or_default());
    if let Some(dir) = rustode_workspace::default_extensions_dir(|key| std::env::var_os(key)) {
        app.workspace_mut().set_extensions_location(&dir);
    }
    if let Some(dir) = rustode_workspace::default_global_storage_dir(|key| std::env::var_os(key)) {
        app.workspace_mut().set_global_storage_location(&dir);
    }
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture);
    let result = app.run(&mut terminal);
    let _ = execute!(
        std::io::stdout(),
        DisableBracketedPaste,
        DisableMouseCapture
    );
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

/// Where `settings.json` and `keybindings.json` live: `RUSTODE_USER_DIR`, or
/// `rustode/User` under the XDG config folder (VS Code's `User` layout).
fn user_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RUSTODE_USER_DIR") {
        return PathBuf::from(dir);
    }
    let config = std::env::var_os("XDG_CONFIG_HOME").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"),
        PathBuf::from,
    );
    config.join("rustode").join("User")
}
