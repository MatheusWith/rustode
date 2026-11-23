//! The CLI seam for the git hooks: run `xtask` against fixture messages and
//! fixture repositories, and assert on exit codes and messages.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn xtask(dir: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .current_dir(dir)
        .env_remove("RUSTODE_ALLOW_MAIN_PUSH")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("xtask runs");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(stdin.as_bytes())
        .expect("stdin is writable");
    child.wait_with_output().expect("xtask finishes")
}

/// A fresh, empty directory under the system temp dir.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustode-xtask-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        // A commit hook runs with git's own variables set. In a linked
        // worktree GIT_INDEX_FILE is absolute, and these fixture
        // repositories would stage into the worktree's index.
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        // A commit hook runs with the committing repository's GIT_* variables;
        // left in, they send these fixture commands to that repository.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn repo_with_commits(name: &str, messages: &[&str]) -> (PathBuf, String) {
    let dir = scratch(name);
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "Test"]);
    git(&dir, &["config", "user.email", "test@example.invalid"]);
    for (i, msg) in messages.iter().enumerate() {
        std::fs::write(dir.join("f"), i.to_string()).expect("write");
        git(&dir, &["add", "f"]);
        git(&dir, &["commit", "-q", "-m", msg]);
    }
    let head = git(&dir, &["rev-parse", "HEAD"]);
    (dir, head)
}

fn check_message(name: &str, message: &str) -> Output {
    let dir = scratch(name);
    std::fs::write(dir.join("MSG"), message).expect("write message");
    xtask(&dir, &["commit-msg", "MSG"], "")
}

const ZERO: &str = "0000000000000000000000000000000000000000";

#[test]
fn commit_msg_accepts_a_conventional_message_with_trailer() {
    let out = check_message(
        "ok",
        "feat(core): add the rope buffer\n\nWhy it matters.\n\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\n# git comment\n",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn commit_msg_rejects_unknown_type_scope_length_and_trailer() {
    for (case, msg, needle) in [
        ("type", "feature: add x\n", "type"),
        ("scope", "feat(editor): add x\n", "scope"),
        ("long", &format!("feat: {}\n", "a".repeat(70)), "72"),
        ("period", "feat: add x.\n", "period"),
        ("upper", "feat: Add x\n", "lower case"),
        (
            "trailer",
            "feat: add x\n\nCo-Authored-By: someone\n",
            "Co-Authored-By",
        ),
    ] {
        let out = check_message(case, msg);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{case}: should be rejected");
        assert!(err.contains(needle), "{case}: stderr was {err}");
        assert!(err.contains("CONTRIBUTING.md"), "{case}: stderr was {err}");
    }
}

#[test]
fn pre_push_accepts_a_typed_branch_with_valid_messages() {
    let (dir, head) = repo_with_commits("push-ok", &["feat(core): add x", "fix(core): fix x"]);
    let line = format!("refs/heads/feat/add-x {head} refs/heads/feat/add-x {ZERO}\n");
    let out = xtask(&dir, &["pre-push"], &line);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn pre_push_rejects_main_bad_branch_names_and_bad_messages() {
    let (dir, head) = repo_with_commits("push-bad", &["feat(core): add x", "wip"]);
    for (case, remote_ref, needle) in [
        ("main", "refs/heads/main", "main"),
        ("name", "refs/heads/my-branch", "branch name"),
        ("message", "refs/heads/feat/add-x", "wip"),
    ] {
        let line = format!("refs/heads/x {head} {remote_ref} {ZERO}\n");
        let out = xtask(&dir, &["pre-push"], &line);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{case}: should be rejected");
        assert!(err.contains(needle), "{case}: stderr was {err}");
    }
}

#[test]
fn pre_push_ignores_branch_deletion() {
    let (dir, _) = repo_with_commits("push-delete", &["feat(core): add x"]);
    let line = format!("(delete) {ZERO} refs/heads/feat/old {ZERO}\n");
    let out = xtask(&dir, &["pre-push"], &line);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
