# ADR 0015: Language servers and debug adapters run as child processes over stdio, supervised by the workspace

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session); the LSP supervisor and diagnostics are implemented in `crates/workspace/src/lsp/` (ticket 29); hover, completion, navigation and the real servers follow (tickets 30 to 32); DAP is M7
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

## Implementation (LSP, ticket 29)

- **Settings.** VS Code has no built-in setting for language servers, so the key is rustode's own, registered in the settings schema as an object (`crates/config/src/settings.rs`): `"rustode.languageServers": { "rust": { "command": "rust-analyzer", "args": [], "initializationOptions": {} } }`, keyed by the workspace's language ids. Each entry is checked on its own (`crates/workspace/src/lsp/settings.rs`); a problem is reported through `Workspace::language_server_problems` and never stops the other entries.
- **Lifecycle.** One server per workspace root and language, started when the first document of that language opens; the root is the opened folder when the file is in it, else the file's folder. The client sends `initialize` (offering the `utf-32`, `utf-8` and `utf-16` position encodings), `initialized`, `didOpen`, `didChange` (the changed range or the whole text, as the server asks), `didSave`, `didClose`, and `shutdown` then `exit` when the workspace is dropped (`crates/workspace/src/lsp/mod.rs`, `server.rs`).
- **Threads.** Each server has a writer thread, a reader thread feeding a channel, and a thread logging its stderr (`crates/workspace/src/lsp/process.rs`). Frontends call `Workspace::poll_language_servers`; the waker set with `Workspace::set_waker` is called by the reader threads (the same waker serves the extensions' background work), so the GUI repaints when diagnostics arrive while it is idle, without polling on a timer. The TUI picks them up on its existing 250 ms idle tick.
- **Supervision.** A crash (the server's output ends without a `shutdown`) is reported in the server's status (starting, running, crashed, stopped) and as an event, its diagnostics are dropped, and the server is restarted at once, up to four times within three minutes; the fifth crash in that window leaves it crashed until "Language Server: Restart" (`rustode.restartLanguageServer`). These numbers are a design choice modeled on `vscode-languageclient`, which is not in the source tree, so they are not a verified VS Code fact (`crates/workspace/src/lsp/server.rs`).
- **Test server.** `rustode-test-lsp` (`crates/test-lsp/`, `publish = false`) is a stdio server for tests only, driven from `crates/workspace/tests/lsp.rs` and the frontend snapshot tests. It reports every `ERROR`, `WARN`, `INFO` and `HINT`, exits on `CRASH`, and logs what it receives.
- **Behavior studied in VS Code** (markers, the problems list and panel, the status bar counts, squiggle colors) is credited in [`ATTRIBUTION.md`](../../ATTRIBUTION.md) and in the doc comments of `crates/workspace/src/lsp/markers.rs`, `crates/tui/src/problems.rs` and `crates/gui/src/problems.rs`.

## Consequences

- Phase 1 needs one process supervisor, used first by the LSP client.
- The validation targets are rust-analyzer and typescript-language-server (M5), and lldb-dap and debugpy (M7).
- Running a language server inside an extension, as VS Code does, waits for an extension host (out of scope for Phase 1).
