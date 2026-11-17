# ADR 0019: Tests are written first, at three seams: the workspace API, input to frame, and the CLI process

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session, seam check included; applied in `crates/workspace/tests/`, `crates/cli/tests/` and `xtask/tests/`)
- **Build:** rustode at `d6debfb`; studied: the source tree of the App (local VSCodium 1.147.0)

## Context

Phase 1 reimplements a large amount of studied behavior. Tests that check internal structures break on every refactor and still miss behavior. A few high seams keep the tests stable and turn the behavior into the specification.

## Decision

Tests exercise external behavior through a public seam and assert on observable results: workspace state, emitted events, rendered cells or pixels, process output and exit codes. They never assert on internal data structures or private functions. They are written before the code, or together with it (red, then green; refactoring belongs to review). The seams are as few and as high as possible:

1. **The workspace public API: the main seam.** Editing, undo grouping, multi-cursor, encodings and line endings, external changes and configuration errors are tested here; later also search, splits, LSP flows (crash and restart included), DAP flows and extension management. Today: `crates/workspace/tests/open_save.rs`, `editing.rs`, `multi_cursor.rs`, `external_changes.rs`, `config.rs` and `stress.rs`.
2. **Input to frame, per frontend.** Key, mouse and resize events are fed in, and the result is snapshotted: with ratatui's test backend for the TUI, at each width band; with egui_kittest for the GUI, including the AccessKit tree. These tests cover presentation only, from M2 and M3 on.
3. **The CLI process.** The binary runs, and the test asserts on its output and exit code (`crates/cli/tests/version.rs`). The git hooks are tested the same way, against fixture commits and branches in temporary repositories (`xtask/tests/hooks.rs`).

Containment rules:

1. External boundaries are replaced by doubles only at the process or network edge: test LSP and DAP servers speaking stdio, a local HTTP stand-in for Open VSX, temporary file systems.
2. Tests are deterministic: no real network, no real home directory, no shared state between tests, and fixed seeds for randomized edits (`crates/workspace/tests/editing.rs:138`, `stress.rs:113`).
3. Tests never touch the owner's profiles, never write to folders 01 to 08, and never send personal data anywhere.
4. The other test layers:
   - every fixed bug gets a regression test at the highest seam that reproduces it;
   - stress tests carry time and memory budgets (`crates/workspace/tests/stress.rs`);
   - property tests check, for example, that any edit sequence followed by a full undo restores the original (`crates/workspace/tests/editing.rs:135-170`);
   - criterion micro-benchmarks gate regressions ([ADR 0026](0026-micro-benchmark-gate.md));
   - the cross-editor comparison runs once per milestone ([ADR 0024](0024-comparison-with-the-app.md)).
5. The commit hook runs the fast tests. Tests named `stress_*` or `slow_*` run only in the full suite (`.config/nextest.toml:4-6`), and the large cases are `#[ignore]`d and run explicitly (`crates/workspace/tests/stress.rs:4-8`).
6. When the expected behavior comes from VS Code's own tests, it is credited ([ADR 0009](0009-mit-and-reimplemented-behavior.md)). Undo grouping, for example, follows `src/vs/editor/test/browser/controller/cursor.test.ts:5925-6240` ([`ATTRIBUTION.md`](../../ATTRIBUTION.md)).

## The guarantee

A refactor that keeps the behavior keeps the tests green, and every behavior claim has a test at a public seam.

## Consequences

- Measured: 133 tests passed and 2 large stress cases were skipped. Command, from the project root at `d6debfb` (2026-11-17): `cargo nextest run --workspace --locked --offline -j 4`.
- The `tdd` skill is the working method for this loop (AGENTS.md §7).
- Seam 2 does not exist yet, because the frontends are placeholders ([ADR 0013](0013-crates-and-workspace-api.md)).
