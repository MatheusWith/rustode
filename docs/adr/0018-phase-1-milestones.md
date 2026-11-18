# ADR 0018: Phase 1 ships in ten milestones, M0 to M9, with a fixed scope and fixed exclusions

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; tracked in [`ROADMAP.md`](../../ROADMAP.md), "Editor: Phase 1 milestones")
- **Build:** rustode at `d6debfb`

## Context

The owner wants an editor with VS Code's ergonomics and VSCodium's open-source, telemetry-free stance, but faster and leaner in every metric that matters to them: cold and warm startup, typing latency, idle and loaded memory, and CPU, GPU and disk use.

Phase 1 includes the core, a CLI, a TUI, a GUI, syntax highlighting, an LSP client, a DAP client, workspace search, split editors, a minimap and extension management. That is far larger than a typical core MVP (estimated from the scope alone). The order of the milestones has to give a usable editor early, and make each later milestone shippable on its own.

## Decision

Phase 1 ships in ten milestones:

| Milestone | Scope |
|---|---|
| M0, pipeline and governance | repository and privacy review; workspace skeleton, pinned toolchain and lints; pre-commit and pre-push hooks; CI with pull-request title checks and coverage gates; cargo deny and cargo audit (Dependabot was in the scope until the owner removed it on 2026-11-18); the contributing guide with the review checklist; the Editor section of `AGENTS.md` and the ADRs; the Wayland virtual-input spike |
| M1, core and CLI | buffer, editing, multi-cursor, undo/redo, encoding and line-ending I/O, external change detection, configuration, tracing, and the CLI (open, version, bench) |
| M2, workspace and TUI | the workspace API, and the TUI with tabs, explorer, command palette, quick open, status bar and find/replace |
| M3, GUI | the same feature set in egui, and the first cross-editor benchmark run |
| M4, syntax | Tree-sitter highlighting, language detection, Dark Modern token colors |
| M5, LSP client | diagnostics, hover, completion, go to definition, references, rename and format; validated with rust-analyzer and typescript-language-server, configured in `settings.json` |
| M6, layout and search | the global search panel with streaming results and a replace preview; split editors; the minimap (GUI only) |
| M7, DAP client | `launch.json`; breakpoints, including conditional ones; run, step and continue; call stack, variables, watch and debug console; validated with lldb-dap and debugpy |
| M8, extension management | Open VSX search, install, uninstall, enable, disable and list. Nothing is loaded. Manifest and storage follow VS Code's format |
| M9, Phase 1 report | a full comparison against the App and a complete Gap Analysis |

Containment rules:

1. Every feature milestone is built test-first ([ADR 0019](0019-tests-at-three-seams.md)). Each one ends with a benchmark run and an updated Gap Analysis in `ROADMAP.md` ([ADR 0024](0024-comparison-with-the-app.md)). From M4 on, when both frontends exist, each feature lands in the TUI and the GUI together.
2. These are out of scope for Phase 1:
   - an extension host, and running any extension code;
   - loading any declarative extension contribution (themes, grammars, snippets, keybindings, language configuration);
   - TextMate grammars;
   - a Web frontend, an MCP server, remote access and the JSON-RPC transport (only the protocol types ship, [ADR 0014](0014-in-process-frontends.md));
   - an integrated terminal and source control;
   - js-debug, and any proprietary debug adapter or language server;
   - the Microsoft Marketplace and Microsoft's proprietary extensions and services, including in tests and fixtures (AGENTS.md §1.2);
   - Windows and macOS ([ADR 0012](0012-linux-x64-only.md));
   - a GUI toolkit other than egui ([ADR 0017](0017-gui-egui-wgpu.md));
   - a custom piece-tree buffer, unless [ADR 0027](0027-ropey-or-piece-tree.md) is accepted.
3. The Extensions view marks every installed extension as "not loaded in this phase", and never contacts the Microsoft Marketplace.
4. The extension manifest model and the storage layout follow the format studied in the source tree, credited per [ADR 0009](0009-mit-and-reimplemented-behavior.md), so that a later extension host needs no migration.

## The guarantee

From M3 on, the editor is usable in both frontends. Each later milestone adds one capability, and its benchmark run and Gap Analysis show where it stands against the App.

## Consequences

- The state of each milestone is tracked in `ROADMAP.md`, "Editor: Phase 1 milestones".
- Anything on the exclusion list needs a new decision (Phase 2 or later).
- Extensions installed in Phase 1 cannot do anything until an extension host exists. Users are told so in the UI (rule 3).
