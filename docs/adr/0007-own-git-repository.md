# ADR 0007: rustode is its own git repository; the evidence folders and private notes stay out of it, and the first push waits for a privacy review

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; the privacy review was approved by the owner on 2026-11-16; applied in `.gitignore`, the local `.git/info/exclude` and the first commit `78aa4d5`)
- **Build:** rustode at `d6debfb`

## Context

Phase 1 builds a Rust editor in the folder that holds the study of the App, so the study becomes the editor's live specification. The editor needs history, pull requests and CI. Three facts shape how the folder is versioned:

- Folders 01 to 08 are copies that `montar.sh` rebuilds from the live sources (AGENTS.md §0.1). Together they take 1.5 GB (measured from the project root: `du -shc 0[1-8]*`).
- Some of them hold personal data, for example `05-qa/results/backup-sharedStorage-125048.vscdb` (AGENTS.md §2.1).
- The owner keeps a prompt file and planning notes (spec and tickets) in the folder. They must never reach GitHub, and they don't belong in the shared ignore file either.

## Decision

The rustode folder is a git repository of its own, with the owner's private GitHub repository as `origin`. The owner's home-directory repository is never modified.

Containment rules:

1. `.gitignore` ignores all of 01 to 08 and every `*.vscdb` file (`.gitignore:1-13`).
2. The prompt file and the planning notes are excluded in the local, unversioned `.git/info/exclude`, never in `.gitignore`. Measured: `git check-ignore -v` names `.git/info/exclude` for both.
3. Nothing is pushed before a privacy review that the owner approves. The review runs gitleaks over the files to be committed, searches for the owner's name, email and absolute home paths, and rewrites absolute home paths as `~/…`.
4. The first push is the owner's to run. The pre-push hook refuses `main` unless `RUSTODE_ALLOW_MAIN_PUSH=1` is set (`xtask/src/pre_push.rs:9-11`, `:24-27`). After that push, `main` changes only through pull requests ([ADR 0022](0022-commits-branches-pull-requests.md)).

## The guarantee

The history holds no evidence copy, no `.vscdb` file and none of the owner's private notes, and nothing left the machine before the owner approved the review.

Check, from the project root: `git ls-files | grep -cE '^0[1-8]|\.vscdb$'` prints `0` (measured at `d6debfb`).

## Consequences

- Measured during the review, whose record stays in the owner's unversioned notes:
  - gitleaks over a temporary copy of the candidate files found no leak;
  - `grep` for absolute home paths and email addresses found no match.

  The owner approved the review, and `78aa4d5` is the first commit.
- As of 2026-11-17 nothing has been pushed: `git status -sb` shows `## main` with no upstream (measured from the project root). CI has never run, and branch protection can only be checked after the push ([`lacunas.md`](../lacunas.md) G19).
- Regenerating the evidence still goes through `montar.sh` on each machine. Cloning the repository does not bring the App or the source tree.
- Anything else that would leave the machine (a release, a package, a mirror) needs a new decision.
