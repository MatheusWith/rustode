# ADR 0022: Commits follow Conventional Commits with closed types and scopes, branches are typed, and main changes only through squash-merged pull requests

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in `xtask/src/conventions.rs`, `committed.toml`, `xtask/src/pre_push.rs` and the `conventions` CI job); server-side branch protection not verified yet
- **Build:** rustode at `d6debfb`

## Context

A consistent history makes reviews and changelogs cheap, and pushing straight to `main` skips review and CI. It is not known whether the owner's plan offers server-side branch protection for a private repository ([`lacunas.md`](../lacunas.md) G19).

## Decision

- **Commit messages** are `type(scope): subject`:
  - the type is one of `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`, `revert`;
  - the scope is optional and comes from a closed list: one per crate, plus `bench`, `deps`, `docs`, `ci` and `hooks`;
  - the subject is imperative, starts in lower case and has no trailing period;
  - the title is at most 72 characters, and body lines too;
  - a co-author trailer reads `Co-Authored-By: Name <email>`.
- **Branch names** are `<type>/<kebab-name>`: a commit type, a slash, and words of lower-case letters and digits joined by `-`.
- **No push to `main`.** Every change reaches `main` through a pull request and is squash-merged, so the pull request title becomes the commit and must itself be a valid message.
- **Branch protection:** find out whether the private repository's plan offers server-side protection. If it does not, document that "no push to main" is a convention enforced by the pre-push hook and CI only.

Containment rules:

1. The rules have one implementation, in `xtask/src/conventions.rs` (types and scopes `:4-26`, title `:29`, subject `:76-88`, trailer `:90-99`, branch `:101-113`). The commit-msg hook, the pre-push hook and CI all use it (`.pre-commit-config.yaml:39-56`; `.github/workflows/ci.yml:66-92`). `committed` checks the same types and scopes, plus imperative mood and line length (`committed.toml:1-12`).
2. The pre-push hook checks the target branch, the branch name and every message in the pushed range (`xtask/src/pre_push.rs:10-34`). `RUSTODE_ALLOW_MAIN_PUSH=1` exists only to bootstrap the repository (`xtask/src/pre_push.rs:9-11`).
3. Every rejection points to [`CONTRIBUTING.md`](../../CONTRIBUTING.md), "Editor code: commits, branches and checks" (`xtask/src/conventions.rs:32`).
4. Every pull request runs the title job; none is exempt. (Dependabot's pull requests were exempt until the owner removed Dependabot on 2026-11-18.)

## The guarantee

Every commit on `main` comes from a reviewed pull request and has a well-formed message.

Check: CI runs `cargo xtask pre-push` over the pull request's range, then `committed --no-merge-commit <base>..<head>` (`.github/workflows/ci.yml:85-92`).

## Consequences

- The xtask checks are tested against fixture repositories at the CLI seam (`xtask/tests/hooks.rs`). They passed at `d6debfb` as part of the full suite (133 passed, 2 skipped; command in [ADR 0019](0019-tests-at-three-seams.md)).
- Pending: branch protection can be checked only after the first push ([ADR 0007](0007-own-git-repository.md); G19).
- A new crate needs its scope added in two places, `xtask/src/conventions.rs` and `committed.toml`.
