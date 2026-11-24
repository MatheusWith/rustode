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

/// Runs the binary with the tracing variables cleared, then set from `env`.
fn rustode_with_env(args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rustode"))
        .args(args)
        .env_remove("RUSTODE_LOG")
        .env_remove("RUSTODE_LOG_FORMAT")
        .envs(env.iter().copied())
        .output()
        .expect("the rustode binary runs")
}

#[test]
fn json_debug_logs_go_to_stderr_and_stdout_stays_clean() {
    let out = rustode_with_env(
        &["--version"],
        &[("RUSTODE_LOG", "debug"), ("RUSTODE_LOG_FORMAT", "json")],
    );
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("rustode {}\n", env!("CARGO_PKG_VERSION"))
    );
    let err = String::from_utf8_lossy(&out.stderr);
    let events: Vec<serde_json::Value> = err
        .lines()
        .map(|line| serde_json::from_str(line).expect("every stderr line is JSON"))
        .collect();
    let started = events
        .iter()
        .find(|event| event["fields"]["message"] == "cli started")
        .expect("a 'cli started' event on stderr");
    assert_eq!(started["level"], "DEBUG");
    assert_eq!(started["fields"]["command"], "Version");
}

#[test]
fn default_level_keeps_stderr_quiet() {
    let out = rustode_with_env(&["--version"], &[]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
}

#[cfg(unix)]
#[test]
fn an_argument_that_is_not_utf8_is_a_usage_error_not_a_crash() {
    use std::os::unix::ffi::OsStrExt;
    for args in [&b"\xff"[..], b"bench\xff"] {
        let out = Command::new(env!("CARGO_BIN_EXE_rustode"))
            .arg(std::ffi::OsStr::from_bytes(args))
            .env_remove("RUSTODE_LOG")
            .output()
            .expect("the rustode binary runs");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {:?}", out.status);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("panicked"), "{err}");
        assert!(err.contains("Usage: rustode"), "{err}");
    }
}

#[test]
fn hostile_arguments_and_log_filters_are_errors_not_crashes() {
    let long = "x".repeat(100_000);
    for arg in ["", " ", "-", "--", "\n", long.as_str()] {
        let out = rustode(&[arg]);
        assert!(
            matches!(out.status.code(), Some(0 | 2)),
            "{}: {:?}",
            arg.len(),
            out.status
        );
        assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
    }
    for filter in ["=====", "[[[", "debug,,,", "a=b=c", "💥"] {
        let out = rustode_with_env(&["--version"], &[("RUSTODE_LOG", filter)]);
        assert!(out.status.success(), "{filter:?}: {:?}", out.status);
        assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
    }
}
