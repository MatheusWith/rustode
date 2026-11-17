//! The `pre-push` hook: target branch, branch name and every pushed message.

use crate::conventions::{check_branch, check_message};
use std::process::Command;

const ZERO: &str = "0000000000000000000000000000000000000000";

/// Commits made before the message checks existed, kept as they are because
/// rewriting them would change every later hash that the documents cite.
/// `a780b8f` is the commit that added the hooks; its title is 75 characters.
const LEGACY: [&str; 1] = ["a780b8f90919fb561d40f38816ec252875bfd1ef"];

/// Checks the ref lines git passes on stdin; returns one line per problem.
/// Setting `RUSTODE_ALLOW_MAIN_PUSH=1` allows pushing `main` (repository bootstrap only).
pub fn check(stdin: &str) -> Result<Vec<String>, String> {
    let allow_main = std::env::var("RUSTODE_ALLOW_MAIN_PUSH").is_ok_and(|v| v == "1");
    let mut problems = Vec::new();
    for line in stdin.lines().filter(|l| !l.trim().is_empty()) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [_, local_sha, remote_ref, remote_sha] = fields[..] else {
            return Err(format!("unexpected pre-push input line: '{line}'"));
        };
        if local_sha == ZERO {
            continue; // deleting a remote branch
        }
        let Some(branch) = remote_ref.strip_prefix("refs/heads/") else {
            continue; // tags and other refs
        };
        if branch == "main" {
            if !allow_main {
                problems.push("pushing to main is not allowed; open a pull request".to_owned());
            }
        } else {
            problems.extend(check_branch(branch));
        }
        problems.extend(check_range(local_sha, remote_sha)?);
    }
    Ok(problems)
}

fn check_range(local_sha: &str, remote_sha: &str) -> Result<Vec<String>, String> {
    let range = if remote_sha == ZERO {
        vec![
            local_sha.to_owned(),
            "--not".to_owned(),
            "--remotes".to_owned(),
        ]
    } else {
        vec![format!("{remote_sha}..{local_sha}")]
    };
    let mut args = vec!["rev-list".to_owned(), "--no-merges".to_owned()];
    args.extend(range);
    let mut problems = Vec::new();
    for sha in git(&args)?.lines().filter(|sha| !LEGACY.contains(sha)) {
        let message = git(&[
            "log".to_owned(),
            "-1".to_owned(),
            "--format=%B".to_owned(),
            sha.to_owned(),
        ])?;
        problems.extend(
            check_message(&message)
                .into_iter()
                .map(|p| format!("commit {}: {p}", &sha[..sha.len().min(10)])),
        );
    }
    Ok(problems)
}

fn git(args: &[String]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
