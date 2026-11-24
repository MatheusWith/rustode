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
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage: rustode-tui PATH..."));
}

#[test]
fn a_missing_file_is_an_error_with_its_path() {
    let out = tui(&["/nonexistent/rustode-tui-test.txt"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("/nonexistent/rustode-tui-test.txt"), "{err}");
}

#[test]
fn version_flags_print_the_version_and_exit_0() {
    for flag in ["--version", "-V"] {
        let out = tui(&[flag]);
        assert_eq!(out.status.code(), Some(0), "{flag}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            text.trim(),
            concat!("rustode-tui ", env!("CARGO_PKG_VERSION")),
            "{flag}"
        );
    }
}

#[test]
fn help_flags_print_usage_and_exit_0() {
    for flag in ["--help", "-h"] {
        let out = tui(&[flag]);
        assert_eq!(out.status.code(), Some(0), "{flag}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("Usage: rustode-tui PATH..."),
            "{flag}"
        );
    }
}

#[test]
fn a_flag_with_other_arguments_is_a_usage_error() {
    for args in [
        &["--version", "file.txt"][..],
        &["file.txt", "--help"],
        &["--bogus"],
        &["-x", "file.txt"],
    ] {
        let out = tui(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("unexpected argument"), "{args:?}: {err}");
        assert!(
            err.contains("Usage: rustode-tui PATH..."),
            "{args:?}: {err}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_is_not_a_crash() {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::OsStr::from_bytes(b"/nonexistent/\xff.txt");
    let out = Command::new(env!("CARGO_BIN_EXE_rustode-tui"))
        .arg(path)
        .output()
        .expect("the rustode-tui binary runs");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn hostile_arguments_and_log_filters_are_errors_not_crashes() {
    let long = "x".repeat(100_000);
    for arg in ["", " ", "-", "--", "\n", long.as_str()] {
        let out = tui(&[arg]);
        assert!(
            matches!(out.status.code(), Some(1 | 2)),
            "{}: {:?}",
            arg.len(),
            out.status
        );
        assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
    }
    for filter in ["=====", "[[[", "debug,,,", "a=b=c", "💥"] {
        let out = Command::new(env!("CARGO_BIN_EXE_rustode-tui"))
            .arg("--version")
            .env("RUSTODE_LOG", filter)
            .output()
            .expect("the rustode-tui binary runs");
        assert!(out.status.success(), "{filter:?}: {:?}", out.status);
        assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
    }
}
