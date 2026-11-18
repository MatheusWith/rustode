# ADR 0023: The same checks run in git hooks and in CI, and the security scans run offline

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in `.pre-commit-config.yaml`, `.github/workflows/ci.yml` and `.semgrep/`); the Semgrep registry rules in CI are not applied
- **Build:** rustode at `d6debfb`

## Context

Hooks can be skipped with `--no-verify`. CI catches that only if it runs the same checks. Security scans should work without a network and without sending usage data. Semgrep's registry rules may not be redistributed, so they cannot be vendored into the repository (commit `a780b8f`).

## Decision

| When | Checks |
|---|---|
| commit | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo nextest` (fast profile), `gitleaks` on the staged changes, `semgrep` with the project rules |
| commit message | `committed` and `cargo xtask commit-msg` |
| push | branch name and target, every message in the pushed range, the full `nextest` suite, `clippy`, `cargo deny` |
| pull request (CI) | all of the above on Linux x64, plus the pull request title, the coverage gates, `cargo audit`, `gitleaks` over the history and the Semgrep rule tests |

Dependency updates are made by hand, in ordinary pull requests that pass the same checks. Dependabot opened weekly updates for crates and GitHub Actions until the owner removed it on 2026-11-18; `cargo deny` and `cargo audit` still flag vulnerable or disallowed dependencies.

Semgrep runs offline with metrics off, using rules written for this project. Because the registry rules cannot be vendored, they may run only in CI.

Containment rules:

1. Every hook is `repo: local` with `language: system`, so no hook repository is downloaded (`.pre-commit-config.yaml:1-8`).
2. Semgrep runs with `--metrics=off --disable-version-check`, locally (`.pre-commit-config.yaml:33-37`) and in CI (`.github/workflows/ci.yml:60-64`). Its rules have tests (`.semgrep/rust.rs`, run with `semgrep scan --test`).
3. CI builds with `--locked` and `RUSTFLAGS: -D warnings` (`.github/workflows/ci.yml:18`, `:33-35`).
4. Nobody bypasses the hooks with `--no-verify` ([`CONTRIBUTING.md`](../../CONTRIBUTING.md)).

## The guarantee

Nothing reaches `main` that would fail a local hook, and no scan sends data out.

## Consequences

- Measured at `d6debfb` on 2026-11-17, from the project root:
  - `semgrep scan --test --config .semgrep/rust.yml .semgrep/rust.rs --metrics=off --disable-version-check` reported `4/4: ✓ All tests passed`;
  - `semgrep scan --config .semgrep --metrics=off --disable-version-check --error --quiet .` exited 0, with no findings.
- Not applied: the decision allows the registry rules in CI, but CI runs only the project rules (`.github/workflows/ci.yml:60-64`).
- CI has never run, because the repository has not been pushed ([ADR 0007](0007-own-git-repository.md)).
- CI pins gitleaks 8.28.0 (`.github/workflows/ci.yml:58`). The local gitleaks is whatever is installed (`gitleaks version` prints `version is set by build process`), so the two may differ.
- The micro-benchmark gate ([ADR 0026](0026-micro-benchmark-gate.md)) runs in CI on pull requests (the `bench-gate` job of `.github/workflows/ci.yml`) and, only when `RUSTODE_BENCH_GATE=1` is set, in the pre-push hook (`.pre-commit-config.yaml`).
