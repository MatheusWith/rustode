//! The command line of `rustode bench`.

use std::path::PathBuf;

use super::Failure;

/// `rustode bench --help`.
pub(crate) const USAGE: &str = "\
Usage: rustode bench [OPTIONS] <SCENARIO>
       rustode bench --list

Runs one scenario of the versioned scenario list on rustode and on the App
(the local VSCodium build in 01-app/) and writes a labeled report to a new run
folder. The App runs from a verified copy, on the QA harness's virtual display,
with an isolated profile and no network; one App at a time.

Options:
  --dry-run             print the plan; launch nothing, write nothing
  --list                list the scenarios and exit
  --side <SIDE>         both (default), rustode or app
  --scenarios <FILE>    the scenario list [default: bench/scenarios.toml]
  --out <DIR>           where run folders go [default: target/bench]
  --project <DIR>       the folder with 01-app/ and 02-package-analysis/ [default: .]
  --app-copy-dir <DIR>  where the App copy and profiles go, on disk [default: the run folder]
  --gui-bin <FILE>      the rustode GUI for launch scenarios [default: rustode-gui next to this binary]
  --no-app-trace        do not record the Chromium trace of the App's first presented frame
                        (the control run that bounds the trace's overhead)
  -h, --help            print this help

Environment:
  RUSTODE_QA_ROOT       the QA harness [default: ~/Projects/vscode/qa]
  RUSTODE_NODE          Node for the App driver [default: ~/.local/opt/node-v24.22.1-linux-x64/bin/node]
  RUSTODE_BENCH_LOCK    the one-App-at-a-time lock [default: $XDG_RUNTIME_DIR/rustode-bench.lock]

Exit codes:
  0  the run finished and the report is written
  1  the run failed; its folder is renamed <run>-INVALIDA
  2  usage error
  3  App integrity check failed (a file differs from the manifest or is not in it);
     after a run, its folder is renamed <run>-INVALIDA
  4  refused: another App or bench is running, or not enough free memory
  5  aborted by the memory watchdog; the folder is renamed <run>-INTERROMPIDA
";

/// Which sides of a scenario run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    /// rustode, then the App.
    Both,
    /// rustode only.
    Rustode,
    /// The App only.
    App,
}

/// What the command line asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Print the help.
    Help,
    /// List the scenarios.
    List,
    /// Print the plan only.
    DryRun,
    /// Run the scenario.
    Run,
}

/// Parsed options.
#[derive(Debug)]
pub(crate) struct Options {
    /// What to do.
    pub mode: Mode,
    /// Which sides run.
    pub side: Side,
    /// The scenario list.
    pub scenarios: PathBuf,
    /// The parent of run folders.
    pub out: PathBuf,
    /// The project with the evidence folders.
    pub project: PathBuf,
    /// Where the App copy goes, if not in the run folder.
    pub app_copy_dir: Option<PathBuf>,
    /// The rustode GUI binary, if not `rustode-gui` next to this one.
    pub gui_bin: Option<PathBuf>,
    /// The scenario id.
    pub scenario: Option<String>,
    /// Leave out the App's Chromium trace even if the scenario asks for it.
    pub no_app_trace: bool,
}

/// Parses the arguments after `bench`. A repeated option keeps its last value.
pub(crate) fn parse(args: &[String]) -> Result<Options, Failure> {
    let mut options = Options {
        mode: Mode::Run,
        side: Side::Both,
        scenarios: PathBuf::from("bench/scenarios.toml"),
        out: PathBuf::from("target/bench"),
        project: PathBuf::from("."),
        app_copy_dir: None,
        gui_bin: None,
        scenario: None,
        no_app_trace: false,
    };
    let (mut help, mut list, mut dry_run) = (false, false, false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_owned())),
            _ => (arg.as_str(), None),
        };
        match flag {
            "-h" | "--help" => help = true,
            "--list" => list = true,
            "--dry-run" => dry_run = true,
            "--no-app-trace" => options.no_app_trace = true,
            "--side" | "--scenarios" | "--out" | "--project" | "--app-copy-dir" | "--gui-bin" => {
                let value = inline
                    .or_else(|| rest.next().cloned())
                    .ok_or_else(|| Failure::Usage(format!("{flag} needs a value")))?;
                set(&mut options, flag, value)?;
            }
            _ if flag.starts_with('-') => {
                return Err(Failure::Usage(format!(
                    "unexpected option '{arg}' (see rustode bench --help)"
                )));
            }
            _ => {
                if options.scenario.replace(arg.clone()).is_some() {
                    return Err(Failure::Usage("give one scenario per run".to_owned()));
                }
            }
        }
    }
    options.mode = match (help, list, dry_run) {
        (true, _, _) => Mode::Help,
        (_, true, _) => Mode::List,
        (_, _, true) => Mode::DryRun,
        _ => Mode::Run,
    };
    if matches!(options.mode, Mode::DryRun | Mode::Run) && options.scenario.is_none() {
        return Err(Failure::Usage(
            "which scenario? (see rustode bench --list)".to_owned(),
        ));
    }
    Ok(options)
}

fn set(options: &mut Options, flag: &str, value: String) -> Result<(), Failure> {
    match flag {
        "--side" => {
            options.side = match value.as_str() {
                "both" => Side::Both,
                "rustode" => Side::Rustode,
                "app" => Side::App,
                other => {
                    return Err(Failure::Usage(format!(
                        "--side takes both, rustode or app, not '{other}'"
                    )));
                }
            };
        }
        "--scenarios" => options.scenarios = PathBuf::from(value),
        "--out" => options.out = PathBuf::from(value),
        "--project" => options.project = PathBuf::from(value),
        "--gui-bin" => options.gui_bin = Some(PathBuf::from(value)),
        _ => options.app_copy_dir = Some(PathBuf::from(value)),
    }
    Ok(())
}
