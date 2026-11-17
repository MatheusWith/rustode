//! The `rustode-gui` process: arguments, exit codes and errors. No test here
//! reaches the window, and every child runs without a display, so nothing
//! can open on the desktop.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rustode-gui"))
        .args(args)
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .output()
        .expect("rustode-gui runs")
}

#[test]
fn version_prints_the_version() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("rustode-gui {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn help_prints_the_usage() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("Usage: rustode-gui PATH..."));
}

#[test]
fn a_missing_argument_is_a_usage_error() {
    for args in [&[][..], &["--frobnicate"], &["a.txt", "--version"]] {
        let out = run(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("Usage: rustode-gui"));
    }
}

#[test]
fn a_file_that_cannot_be_read_fails_before_any_window() {
    let missing = std::env::temp_dir().join(format!(
        "rustode-gui-cli-{}-missing.txt",
        std::process::id()
    ));
    let out = run(&[missing.to_str().expect("utf-8 path")]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error:") && stderr.contains("missing.txt"),
        "{stderr}"
    );
}

#[test]
fn several_paths_are_opened_and_each_must_exist() {
    let dir = std::env::temp_dir().join(format!("rustode-gui-cli-{}-paths", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let missing = dir.join("missing.txt");
    let out = run(&[
        dir.to_str().expect("utf-8 path"),
        missing.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(out.status.code(), Some(1), "a folder and a missing file");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error:") && stderr.contains("missing.txt"),
        "{stderr}"
    );
}
