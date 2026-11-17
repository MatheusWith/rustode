# ADR 0014: The frontends link the workspace in-process, send it commands and render its state; other transports come later through the protocol types

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in `crates/workspace/src/lib.rs`)
- **Build:** rustode at `d6debfb`

## Context

The TUI and the GUI must go from input to frame in under 16 ms at p99 ([ADR 0016](0016-tui-ratatui.md), [ADR 0017](0017-gui-egui-wgpu.md)). A process hop on every keystroke would add serialization and scheduling to every frame. Web, MCP and remote frontends, planned for later phases, cannot link Rust code in-process at all.

## Decision

The boundary is hybrid:

- the TUI and the GUI link `rustode-workspace` in-process;
- Web, MCP and remote frontends will use a protocol layer generated from the same types (`rustode-protocol`), in a later phase.

The workspace exposes commands (open, edit, undo, search, split, LSP request, debug action, extension action) and emits events. The frontends render from the workspace's state and never hold business logic.

Containment rules:

1. Every operation a frontend performs is a method of `Workspace`, for example `open`, `save`, `type_text`, `undo` and `resolve_conflict` (`crates/workspace/src/lib.rs:122-348`, `crates/workspace/src/external.rs`).
2. Every change the workspace makes is reported as an `Event`, collected with `take_events` (`crates/workspace/src/lib.rs:49-90`, `:327-330`). `Event` is `#[non_exhaustive]` (`:51`), so adding an event does not break a frontend.
3. A frontend never decides an editing rule, such as how undo steps group or what happens on a conflict. It calls the workspace and renders the result. The review checklist checks this ([`CONTRIBUTING.md`](../../CONTRIBUTING.md)).
4. In Phase 1, `rustode-protocol` ships types only, with no JSON-RPC transport.

## The guarantee

The same input produces the same document state in every frontend, because only the workspace changes state. Behavior is tested once, at the workspace seam ([ADR 0019](0019-tests-at-three-seams.md)).

## Consequences

- Frontend tests cover presentation only, through snapshots.
- Commands and events must stay serializable even though no transport ships in Phase 1. Measured at `d6debfb`: they are not serializable yet. `crates/workspace/Cargo.toml` has no serialization dependency, and `crates/protocol/src/lib.rs` is a one-line placeholder.
- A command or event that cannot be serialized needs a new decision.
