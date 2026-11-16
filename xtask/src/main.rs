//! Project tasks, run with `cargo xtask <task>`. Tasks are added by later tickets.

use std::process::ExitCode;

const USAGE: &str = "Usage: cargo xtask <task>\n\nTasks:\n  help    show this message\n";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None | Some("help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprint!("unknown task: {other}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
