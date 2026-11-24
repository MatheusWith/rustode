//! The `rustode` command-line interface.

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

mod bench;

use std::ffi::OsString;
use std::process::ExitCode;

const USAGE: &str = "\
Usage: rustode [--version | --help]
       rustode bench [OPTIONS] <SCENARIO>    (see rustode bench --help)";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Version,
    Help,
    Bench(Vec<String>),
    Unknown(String),
}

/// An argument that is not UTF-8 is shown lossily and is never a crash.
fn parse(mut args: impl Iterator<Item = OsString>) -> Command {
    let Some(first) = args.next() else {
        return Command::Help;
    };
    match first.to_str() {
        Some("--version" | "-V") => Command::Version,
        Some("--help" | "-h") => Command::Help,
        Some("bench") => {
            Command::Bench(args.map(|arg| arg.to_string_lossy().into_owned()).collect())
        }
        _ => Command::Unknown(first.to_string_lossy().into_owned()),
    }
}

fn main() -> ExitCode {
    if let Err(err) = rustode_workspace::trace::init_from_env() {
        eprintln!("warning: {err}");
    }
    let command = parse(std::env::args_os().skip(1));
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
        Command::Bench(args) => bench::run(&args),
        Command::Unknown(arg) => {
            eprintln!("error: unexpected argument '{arg}'\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
