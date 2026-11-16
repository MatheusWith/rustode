//! The CLI seam: run the binary and assert on its output and exit code.

use std::process::Command;

fn rustode(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rustode"))
        .args(args)
        .output()
        .expect("the rustode binary runs")
}

#[test]
fn version_flag_prints_name_and_version() {
    let out = rustode(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("rustode {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_argument_fails_with_usage_on_stderr() {
    let out = rustode(&["--no-such-flag"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--no-such-flag"), "stderr: {err}");
    assert!(err.contains("Usage: rustode"), "stderr: {err}");
}
