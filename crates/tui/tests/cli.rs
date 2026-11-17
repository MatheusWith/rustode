//! The binary at the process seam: usage and errors, without a terminal.

use std::process::Command;

fn tui(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rustode-tui"))
        .args(args)
        .output()
        .expect("the rustode-tui binary runs")
}

#[test]
fn no_file_prints_usage_and_exits_2() {
    let out = tui(&[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage: rustode-tui FILE"));
}

#[test]
fn a_missing_file_is_an_error_with_its_path() {
    let out = tui(&["/nonexistent/rustode-tui-test.txt"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("/nonexistent/rustode-tui-test.txt"), "{err}");
}
