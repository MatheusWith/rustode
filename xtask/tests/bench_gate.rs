//! The CLI seam of `cargo xtask bench-gate`. Only refusals are tested here:
//! a real run measures two builds for minutes, so it is never part of the
//! test suite (the comparison itself is unit-tested in `xtask/src/bench_gate.rs`).

use std::process::Command;

#[test]
fn bench_gate_refuses_an_invalid_threshold_before_measuring() {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["bench-gate", "--threshold", "ten"])
        .output()
        .expect("xtask runs");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("--threshold takes a percentage"),
        "{stderr}"
    );
}
