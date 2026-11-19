//! The `rustode-gui` binary: opens files in tabs and the first folder in
//! the explorer, as `rustode-tui` does, with the user's settings
//! (`RUSTODE_USER_DIR`, else `$XDG_CONFIG_HOME/rustode/User`).

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Usage: rustode-gui PATH... | --version | --help";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Open(Vec<PathBuf>),
    Version,
    Help,
    Usage(String),
}

fn parse(args: impl Iterator<Item = String>) -> Command {
    let args: Vec<String> = args.collect();
    match args.first().map(String::as_str) {
        None => return Command::Usage("missing PATH".to_owned()),
        Some("--version" | "-V") if args.len() == 1 => return Command::Version,
        Some("--help" | "-h") if args.len() == 1 => return Command::Help,
        _ => {}
    }
    if let Some(flag) = args.iter().find(|arg| arg.starts_with('-')) {
        return Command::Usage(format!("unexpected argument '{flag}'"));
    }
    Command::Open(args.into_iter().map(PathBuf::from).collect())
}

fn main() -> ExitCode {
    if let Err(err) = rustode_workspace::trace::init_from_env() {
        eprintln!("warning: {err}");
    }
    match parse(std::env::args().skip(1)) {
        Command::Version => {
            println!("rustode-gui {}", env!("CARGO_PKG_VERSION"));
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
    let mut workspace = rustode_workspace::Workspace::new();
    // rustode's own extensions and global storage folders, never a VS Code
    // profile's.
    if let Some(dir) = rustode_workspace::default_extensions_dir(|key| std::env::var_os(key)) {
        workspace.set_extensions_location(&dir);
    }
    if let Some(dir) = rustode_workspace::default_global_storage_dir(|key| std::env::var_os(key)) {
        workspace.set_global_storage_location(&dir);
    }
    let mut app = match rustode_gui::EditorApp::open_paths_in(workspace, paths) {
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
    match rustode_gui::run(app) {
        Ok(latency) => {
            if let Some(report) = latency {
                eprintln!("rustode-gui: input to present: {report}");
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Where `settings.json` and `keybindings.json` live: `RUSTODE_USER_DIR`, or
/// `rustode/User` under the XDG config folder (VS Code's `User` layout), as
/// for the terminal editor.
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
