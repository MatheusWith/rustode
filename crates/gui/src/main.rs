//! The `rustode-gui` binary: opens files in tabs and the first folder in
//! the explorer, as `rustode-tui` does.

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
    let app = match rustode_gui::EditorApp::open_paths(paths) {
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
