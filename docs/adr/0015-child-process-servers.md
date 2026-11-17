# ADR 0015: Language servers and debug adapters run as child processes over stdio, supervised by the workspace

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session); not implemented yet (M5 for LSP, M7 for DAP)
- **Build:** rustode at `d6debfb`; studied: the source tree of the App (local VSCodium 1.147.0)

## Context

Language servers (rust-analyzer, typescript-language-server) and debug adapters (lldb-dap, debugpy) are separate programs. In VS Code, two different parts start them:

- Language clients live in extensions. One example is the built-in JSON extension, which uses `vscode-languageclient/node` (`extensions/json-language-features/client/src/node/jsonClientMain.ts:8`). Extensions run in the extension host, and Phase 1 has no extension host ([ADR 0018](0018-phase-1-milestones.md)).
- Executable debug adapters are spawned by the workbench and spoken to over their streams (`src/vs/workbench/contrib/debug/node/debugAdapter.ts:239`, `:313`).

A crashed server must not take the editor down, and it must be restartable without restarting the editor.

## Decision

Language servers and debug adapters run as child processes over stdio. The workspace supervises them: it starts them, detects crashes and restarts them. The LSP client (M5) proves child-process management before the DAP client (M7) depends on it.

Containment rules:

1. Servers and adapters are configured in VS Code's format: language servers in `settings.json`, launch configurations in `launch.json`.
2. Every message is traced with the `child_message` span (`crates/trace/src/spans.rs:61`).
3. Tests use test servers and adapters that speak stdio, at the workspace seam ([ADR 0019](0019-tests-at-three-seams.md)).
4. Supervision lives in the workspace, never in a frontend ([ADR 0014](0014-in-process-frontends.md)).
5. No proprietary server or adapter is used, tested or bundled, and js-debug is out of scope (AGENTS.md §1.2; [ADR 0018](0018-phase-1-milestones.md)).
6. Behavior studied in VS Code for these clients (for example how the debug adapter is spawned) is credited in [`ATTRIBUTION.md`](../../ATTRIBUTION.md) when it is implemented ([ADR 0009](0009-mit-and-reimplemented-behavior.md)).

## The guarantee

A crashed language server or debug adapter is reported and can be restarted without restarting the editor.

## Consequences

- Phase 1 needs one process supervisor, used first by the LSP client.
- The validation targets are rust-analyzer and typescript-language-server (M5), and lldb-dap and debugpy (M7).
- Running a language server inside an extension, as VS Code does, waits for an extension host (out of scope for Phase 1).
