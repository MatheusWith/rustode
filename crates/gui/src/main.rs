//! The `rustode-gui` binary: opens one file in an editor window.

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Usage: rustode-gui <file> | --version | --help";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Open(PathBuf),
    Version,
    Help,
    Usage(String),
}

fn parse(mut args: impl Iterator<Item = String>) -> Command {
    let command = match args.next().as_deref() {
        Some("--version" | "-V") => Command::Version,
        Some("--help" | "-h") => Command::Help,
        None => return Command::Usage("missing <file>".to_owned()),
        Some(flag) if flag.starts_with('-') => {
            return Command::Usage(format!("unexpected argument '{flag}'"));
        }
        Some(path) => Command::Open(PathBuf::from(path)),
    };
    match args.next() {
        Some(extra) => Command::Usage(format!("unexpected argument '{extra}'")),
        None => command,
    }
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
        Command::Open(path) => open(&path),
    }
}

fn open(path: &std::path::Path) -> ExitCode {
    let app = match rustode_gui::EditorApp::open(path) {
        Ok(app) => app,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
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
