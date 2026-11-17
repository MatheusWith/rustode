# ADR 0010: Mature ecosystem crates are allowed; "from scratch" applies only to VS Code code

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in `Cargo.toml` `[workspace.dependencies]` and `deny.toml`)
- **Build:** rustode at `d6debfb`

## Context

Rewriting a rope, a lossless encoder, a file watcher or a GPU text renderer would take months and bring back bugs that mature crates have already fixed. [ADR 0009](0009-mit-and-reimplemented-behavior.md) restricts the use of VS Code's code; it says nothing about open-source Rust crates. Those crates must still have licenses compatible with MIT, and their security advisories must be tracked.

## Decision

The editor may depend on mature ecosystem crates, for example ropey, tree-sitter, ratatui, egui/eframe/wgpu, tracing, encoding_rs, notify, ignore and the ripgrep grep crates.

Containment rules:

1. Versions are declared once, in `[workspace.dependencies]` (`Cargo.toml:12-23`), and pinned by `Cargo.lock`. Builds in the hooks and CI use `--locked`.
2. `cargo deny --locked check` (`deny.toml`) does four things:
   - accepts only the licenses listed in `deny.toml:14-26`;
   - denies yanked crates and wildcard versions (`deny.toml:7`, `:33`);
   - denies unknown registries and git sources (`deny.toml:36-39`);
   - checks the advisory database.

   It runs on push and in CI ([ADR 0023](0023-same-checks-local-and-ci.md)). CI also runs `cargo audit`.
3. Each direct dependency is listed in [`ATTRIBUTION.md`](../../ATTRIBUTION.md), "Editor dependencies", with its version, its license and what it is for.
4. A third-party crate is used only by the rustode crate that owns its concern, so that it can be replaced without touching the frontends. For example, ropey is private to `rustode_core::Buffer` (`crates/core/src/buffer.rs:4`, `:9-10`).
5. No dependency brings in Microsoft's proprietary extensions or services (AGENTS.md §1.2).

## The guarantee

Every crate in the build has a license from the list and no known advisory as of the last check.

Measured at `d6debfb` on 2026-11-17, from the project root: `cargo deny --locked --offline check` reported `advisories ok, bans ok, licenses ok, sources ok`. With `--offline`, the advisories were checked against the local copy of the advisory database (`~/.cargo/advisory-dbs/`, folder last modified on 2026-11-17 at 08:24).

## Consequences

- Measured: 6 direct third-party crates, and 56 third-party packages in the resolved graph for all targets. Command, from the project root: `cargo metadata --format-version 1 --locked --offline`.
- `unused-allowed-license = "allow"` keeps licenses that no crate uses yet on the list, as policy for future crates (`deny.toml:12-13`, commit `7ee96c3`).
- A crate whose license is not on the list (for example a GPL crate) needs a new decision, not an edit of `deny.toml`.
