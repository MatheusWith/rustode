# ADR 0021: Clippy runs pedantic with warnings denied and complexity limits; every exception is justified

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in `Cargo.toml:73-84`, `clippy.toml:1-3`, the hooks and CI)
- **Build:** rustode at `d6debfb`

## Context

Lint warnings that pile up stop being read. Small functions with few arguments keep the reimplemented behavior readable and reviewable.

## Decision

The lint setup has four parts:

- `cargo clippy --workspace --all-targets -- -D warnings`, with `pedantic` on;
- cognitive complexity at most 15, functions at most 80 lines, at most 6 arguments;
- `unsafe_code` forbidden;
- `missing_docs`, `dbg_macro`, `todo` and `unwrap_used` warned, so `-D warnings` turns them into errors.

Containment rules:

1. The lint levels live in `[workspace.lints]` (`Cargo.toml:73-84`), and the thresholds in `clippy.toml:1-3`. Every crate opts in with `[lints] workspace = true`.
2. An exception is an `#[allow(...)]` on the smallest item that needs it, with its reason on the same line (for example `xtask/src/coverage.rs:73`).
3. The commit hook, the push hook and CI run clippy with `-D warnings` (`.pre-commit-config.yaml:16-21`, `:65-70`; `.github/workflows/ci.yml:35`). CI also sets `RUSTFLAGS: -D warnings` for every build (`.github/workflows/ci.yml:18`).

## The guarantee

`main` has no clippy warning, and every allowed lint says why it is allowed.

## Consequences

- `forbid` cannot be overridden by an `allow` further down, so a rustode crate cannot use `unsafe` without changing this decision (`Cargo.toml:74`). Foreign-function calls stay inside third-party crates.
- A new toolchain that adds lints means fixing the new warnings in the same change ([ADR 0011](0011-english-and-pinned-toolchain.md)).

## Addendum (2026-11-23): citations re-measured

The line numbers of this ADR were re-measured at `8c3c9ab`: the lint tables moved to `Cargo.toml:73-84` (`unsafe_code` is line 74), the push hook's clippy entry to `.pre-commit-config.yaml:65-70` and CI's clippy step to `.github/workflows/ci.yml:35`. The decision is unchanged.
