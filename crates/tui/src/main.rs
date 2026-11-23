//! `rustode-tui PATH...` (or `--version`, `--help`): the terminal editor. Files open in tabs; the
//! first folder opens in the explorer.
//!
//! Logs go to stderr (`RUSTODE_LOG`, `RUSTODE_LOG_FORMAT`); redirect it to a
//! file when logging, since the editor owns the terminal. On exit the
//! key-to-frame latency is logged at `info`.
//!
//! Extensions live in rustode's own folder (`RUSTODE_EXTENSIONS_DIR`, else
//! `rustode/extensions` under the XDG data folder), and which are disabled
//! in `globalStorage/state.json` in the user folder.

use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use ratatui::crossterm::execute;
use rustode_tui::App;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Usage: rustode-tui PATH... | --version | --help";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Open(Vec<PathBuf>),
    Version,
    Help,
    Usage(String),
}

/// `--version`/`-V` and `--help`/`-h` count only as the sole argument; any
/// other argument starting with `-` is a usage error, as in `rustode-gui`.
/// Paths stay `OsString`s: a path need not be UTF-8.
fn parse(args: impl Iterator<Item = OsString>) -> Command {
    let args: Vec<OsString> = args.collect();
    match args.first().and_then(|arg| arg.to_str()) {
        None if args.is_empty() => return Command::Usage("missing PATH".to_owned()),
        Some("--version" | "-V") if args.len() == 1 => return Command::Version,
        Some("--help" | "-h") if args.len() == 1 => return Command::Help,
        _ => {}
    }
    if let Some(flag) = args
        .iter()
        .find(|arg| arg.to_string_lossy().starts_with('-'))
    {
        return Command::Usage(format!("unexpected argument '{}'", flag.to_string_lossy()));
    }
    Command::Open(args.into_iter().map(PathBuf::from).collect())
}

fn main() -> ExitCode {
    if let Err(err) = rustode_workspace::trace::init_from_env() {
        eprintln!("warning: {err}");
    }
    match parse(std::env::args_os().skip(1)) {
        Command::Version => {
            println!("rustode-tui {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Command::Usage(problem) => {
            eprintln!("error: {problem}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Command::Open(paths) => open(&paths),
    }
}

fn open(paths: &[PathBuf]) -> ExitCode {
    let mut app = match App::open_paths(paths) {
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
