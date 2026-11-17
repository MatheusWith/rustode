//! Project tasks, run with `cargo xtask <task>`.

mod bench_gate;
mod conventions;
mod coverage;
mod pre_push;

use std::io::Read as _;
use std::process::ExitCode;

const USAGE: &str = "\
Usage: cargo xtask <task>

Tasks:
  help              show this message
  commit-msg <file> check a commit message (git commit-msg hook)
  pre-push          check refs read from stdin (git pre-push hook)
  coverage          run the instrumented suite and check the coverage gates
  bench-gate [--base <rev>] [--threshold <percent>] [--rounds <n>]
                    measure the micro-benchmarks of <rev> and of this tree
                    <n> times each on this machine; fail on a slowdown above
                    <percent> (defaults: main, 5, 2)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut guide = conventions::GUIDE;
    let result = match args.first().map(String::as_str) {
        None | Some("help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("commit-msg") => commit_msg(args.get(1)),
        Some("pre-push") => read_stdin().and_then(|input| pre_push::check(&input)),
        Some("coverage") => coverage::run(),
        Some("bench-gate") => {
            guide = bench_gate::GUIDE;
            bench_gate::run(&args[1..])
        }
        Some(other) => {
            eprint!("unknown task: {other}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    report(result, guide)
}

fn commit_msg(path: Option<&String>) -> Result<Vec<String>, String> {
    let path = path.ok_or("commit-msg needs the message file path")?;
    let message = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    Ok(conventions::check_message(&message))
}

fn read_stdin() -> Result<String, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("cannot read stdin: {e}"))?;
    Ok(input)
}

/// Exit code 0 when there is no problem, 1 with problems (each one printed,
/// then `guide`), and 2 when the task could not run.
fn report(result: Result<Vec<String>, String>, guide: &str) -> ExitCode {
    match result {
        Ok(problems) if problems.is_empty() => ExitCode::SUCCESS,
        Ok(problems) => {
            for p in &problems {
                eprintln!("error: {p}");
            }
            eprintln!("{guide}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
