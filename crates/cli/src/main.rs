//! The `rustode` command-line interface.

use std::process::ExitCode;

const USAGE: &str = "Usage: rustode [--version | --help]";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Version,
    Help,
    Unknown(String),
}

fn parse(mut args: impl Iterator<Item = String>) -> Command {
    match args.next().as_deref() {
        Some("--version" | "-V") => Command::Version,
        None | Some("--help" | "-h") => Command::Help,
        Some(other) => Command::Unknown(other.to_owned()),
    }
}

fn main() -> ExitCode {
    if let Err(err) = rustode_workspace::trace::init_from_env() {
        eprintln!("warning: {err}");
    }
    let command = parse(std::env::args().skip(1));
    tracing::debug!(command = ?command, "cli started");
    match command {
        Command::Version => {
            println!("rustode {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Command::Unknown(arg) => {
            eprintln!("error: unexpected argument '{arg}'\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
