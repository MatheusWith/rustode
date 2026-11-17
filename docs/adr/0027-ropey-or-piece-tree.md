# ADR 0027: Keep ropey for the text buffer; replace it with a custom piece tree only if the benchmarks show it behind VS Code's text model

- **Status:** proposed (2026-11-16, left open by the owner in the Phase 1 grilling session; to be decided from the comparisons of [ADR 0024](0024-comparison-with-the-app.md))
- **Build:** rustode at `d6debfb`; the App: local VSCodium 1.147.0

## Context

rustode's text buffer is ropey 1.6.1 (`crates/core/src/buffer.rs:4`, `:9-10`; version from `cargo metadata --format-version 1 --locked --offline`). VS Code's text model is built on a piece tree, in the source tree:

- `createTextBufferFactory` uses `PieceTreeTextBufferBuilder` (`src/vs/editor/common/model/textModel.ts:51`, `:59-60`);
- the piece tree itself is `src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeBase.ts`, on the red-black tree in `rbTreeBase.ts`.

Measured on rustode alone, in release builds on 2026-11-17. The values and the command are in the doc comments of `crates/workspace/tests/stress.rs` (`:4-6`, `:85`, `:93`, `:102-103`, `:136-137`).

| Scenario | Time | Peak RSS |
|---|---|---|
| open 1 MB | 2 ms | 5 MB |
| open 100 MB | 151 ms | 218 MB |
| open 1 GB (the time includes hashing the bytes on disk) | 2.29 s | 2,208 MB |
| 10k random edits, then full undo, on 1 MB | 9 ms | 9 MB |

No comparison with the App exists yet.

## Decision (proposed)

Keep ropey. Write a custom piece tree only if both of these hold:

- a per-milestone comparison (ADR 0024) shows rustode behind the App in a buffer scenario (opening 1 MB, 100 MB or 1 GB; 10k edits then undo);
- a profile attributes the gap to the rope, not to reading, decoding or hashing the file.

Containment rules:

1. ropey stays private to `rustode_core::Buffer` (`crates/core/src/buffer.rs:9-10`). No other crate names it: `grep -rn -e ropey -e Rope crates/workspace crates/fs` prints nothing. A replacement therefore changes one crate.
2. A replacement passes the same workspace-seam tests, the edit-and-undo property test (`crates/workspace/tests/editing.rs:135-170`) and the stress budgets (`crates/workspace/tests/stress.rs`), unchanged.
3. A custom piece tree would reimplement the structure studied in `pieceTreeBase.ts` (Code - OSS, MIT). It would be credited per [ADR 0009](0009-mit-and-reimplemented-behavior.md).
4. The decision is the owner's, made on measured numbers with repetitions (AGENTS.md §0.4), never on a single run.

## The guarantee

The buffer is replaced only on measured evidence, and a replacement cannot change any behavior seen at the workspace seam.

## Consequences

- Until this ADR is accepted, a custom piece tree stays out of scope for Phase 1 ([ADR 0018](0018-phase-1-milestones.md)).
- Opening 1 GB peaks at about 2.2 times the file's size (2,208 MB for 1,024 MB, computed from the measured values). How much of that is the rope, the decoded text or the disk stamp is not known. If the trigger fires, a memory profile is the first step.
