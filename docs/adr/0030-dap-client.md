# ADR 0030: The debug adapter client starts with a tracer: launch, breakpoints, stop and continue, over the LSP child-process supervision

- **Status:** proposed (2026-11-19, ticket 37); the adapter setting `rustode.debugAdapters` is rustode's own and is the owner's to confirm
- **Build:** rustode after `753a255`; studied: the source tree of the App (local VSCodium 1.147.0)

## Context

Milestone M7 is debugging over the Debug Adapter Protocol ([ADR 0018](0018-phase-1-milestones.md)). [ADR 0015](0015-child-process-servers.md) decided that debug adapters, like language servers, run as child processes over stdio, supervised by the workspace, and that the LSP client proves the supervision first. DAP frames its messages as LSP does (`Content-Length`, a blank line, JSON), so the process, the framing and the traces can be shared.

The first slice is a tracer: one path through every layer, so later tickets (stepping, call stack, variables, console: ticket 38) add to a working client. The path is the one VS Code takes when F5 is pressed (`src/vs/workbench/contrib/debug/browser/debugSession.ts:395-406`, `:1065-1090`): `initialize`, `launch`, and when the adapter says `initialized`, `setBreakpoints` for every file and `configurationDone`.

## Decision

The DAP client lives in `crates/workspace/src/dap/` and reuses the child-process supervision of the language servers, factored out of `lsp/` into `crates/workspace/src/child/` instead of copied: the process with its writer, reader and stderr threads, the waker, the framing, and the shutdown wait and reap steps. The LSP code behaves as before.

Containment rules:

1. **Configuration.** Launch configurations are read from `.vscode/launch.json` of the opened folder (`debugConfigurationManager.ts:646`), in VS Code's format, with `rustode-config`'s JSONC parser. A syntax error, a bad entry or an unreadable file is a problem reported by `Workspace::debug_problems`, never a failure; the entries that are fine still work. Every property of an entry goes to the adapter as the arguments of `launch`, as VS Code does. Only `${workspaceFolder}` and `${workspaceFolderBasename}` are substituted so far.
2. **Which adapter.** VS Code's debuggers come from extensions, and rustode has no extension host. The adapter for a configuration `type` is set in `settings.json`, in the shape of `rustode.languageServers`: `"rustode.debugAdapters": { "lldb": { "command": "lldb-dap", "args": [] } }` (`crates/workspace/src/dap/settings.rs`). A type with no adapter is refused with a message that names the setting. No adapter is bundled and no proprietary one is used (ADR 0015 rule 5).
3. **Session.** One session at a time. `start_debugging`, `continue_debugging` and `stop_debugging` return an error that says why they were refused. Stopping asks for `terminate` when the adapter supports it, else `disconnect` with `terminateDebuggee`, as `debugSession.ts:420-445` does; asked twice, the session is dropped. The adapter's `terminated` event ends the session with a `disconnect`; the adapter's exit code message and the program's exit code are kept as `DebugStatus::message`.
4. **A crashed adapter is reported and not restarted.** Unlike a language server, restarting an adapter would run the program again. The session ends with `the debug adapter exited with code N` and a new session can be started by hand.
5. **Breakpoints** belong to the workspace, by absolute path and zero-based line; DAP's lines are one-based and converted at the boundary (`linesStartAt1`). Setting one while a session runs sends that file's breakpoints again. They do not follow edits and are not saved across runs yet.
6. **Commands** are registered in `COMMANDS` with VS Code's ids and keys: `workbench.action.debug.start` (F5), `workbench.action.debug.continue` (F5 while stopped), `workbench.action.debug.stop` (Shift+F5) and `editor.debug.action.toggleBreakpoint` (F9). `Workspace::debug_key_command` says which of start and continue F5 means now, so the frontends hold no `when` logic. The frontends add a click on the gutter, which calls `Workspace::toggle_breakpoint`.
7. **Adapter messages are hostile input.** Every field is read with a checked accessor, a thread id outside `0..=i32::MAX` and a line below 1 are ignored, texts are cut at 500 characters, a response nobody asked for and an unknown message are ignored, the table of requests waiting for an answer is bounded, and the adapter's own requests (`runInTerminal`) are answered with a refusal so it does not wait for us.
8. **Tests** use `rustode-test-dap` (`crates/test-dap`, `publish = false`), a stdio adapter whose program is a text file (a breakpoint stops it before a line; `CRASH` exits with code 3; `HOSTILE` sends malformed messages). It answers `launch` only after `configurationDone` and sends `initialized` when `launch` arrives, so a client that gets the order wrong fails. The seams are the workspace API (`crates/workspace/tests/dap.rs`), the TUI's test backend (`crates/tui/tests/debug.rs`) and egui_kittest (`crates/gui/tests/frame/debug.rs`).

## The guarantee

A debug adapter that crashes, refuses a request or sends garbage ends or ignores the one message, and never takes the editor down or loses an open document. Confirmed by `an_adapter_that_sends_garbage_does_not_derail_the_session`, `a_crashing_adapter_is_reported_and_the_workspace_goes_on` and `a_launch_the_adapter_refuses_ends_the_session_with_its_reason`.

## Consequences

- Not done, and for later tickets: stepping, the call stack, variables, watch and the debug console (ticket 38); attach requests; compound configurations; the other launch variables; conditional breakpoints and logpoints; breakpoints that follow edits and survive a restart; a picker for the configuration (the first one runs until `select_launch_configuration` is called); mutation tests of the DAP parser (AGENTS.md §11.7.1 rule 7: the parser reads `serde_json::Value`, which is checked by the hostile-adapter test only).
- `cargo xtask` scopes: the new crate has no scope of its own; its commits use `workspace`, as `test-lsp`'s do.
- Anything else that needs a different adapter lifecycle (restart on crash, several sessions) requires a new decision, not an extension of this one.
