# ADR 0013: One crate per module, and the workspace crate is the only rustode crate the frontends depend on

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in `Cargo.toml:3` and commit `569b179`; the frontend rule is enforced by `xtask/tests/frontend_deps.rs` since commit `d6debfb`)
- **Build:** rustode at `d6debfb`; studied: the source tree of the App (local VSCodium 1.147.0)

## Context

The editor needs one UI-agnostic core and several thin frontends: TUI, GUI and CLI now, Web and MCP later. If a frontend reaches into the buffer or the file layer directly, logic leaks into the UI, and each frontend ends up reimplementing it. With one crate per module, each part builds and tests on its own, and the direction of every dependency can be checked.

## Decision

The Cargo workspace has one crate per module (`Cargo.toml:3`: members `crates/*` and `xtask`).

| Crate | Role | State at `d6debfb` |
|---|---|---|
| `rustode-core` | text buffer (ropey), selections and multi-cursor, edit operations, undo/redo with VS Code-like grouping; no I/O | M1 part done |
| `rustode-syntax` | Tree-sitter parsing, incremental reparse, highlight queries, language detection | placeholder |
| `rustode-fs` | file reading and writing, encoding detection and conversion, line endings, file watching; workspace search later (ignore-aware, streaming, cancellable) | done except search |
| `rustode-config` | `settings.json` and `keybindings.json` in VS Code's format; schema validation that reports errors instead of failing | done for M1 |
| `rustode-workspace` | the single in-process API for every frontend. It orchestrates core, syntax, fs and config, and later the LSP client, the DAP client and extension management. It owns documents, editor groups and splits, and the command registry, and it emits state-change events | M1 part done |
| `rustode-protocol` | serializable types for every workspace command and event; no transport in Phase 1 | placeholder |
| `rustode-trace` | shared tracing and metrics setup, including the input-to-frame metric | done |
| `rustode-cli` | the `rustode` binary: version and help today; open and bench later | partial |
| `rustode-tui` | terminal frontend ([ADR 0016](0016-tui-ratatui.md)) | placeholder |
| `rustode-gui` | graphical frontend ([ADR 0017](0017-gui-egui-wgpu.md)) | placeholder |
| `xtask` | project tasks: `commit-msg`, `pre-push`, `coverage`; benchmark comparison and release later | partial |

Containment rules:

1. **The frontends (`rustode-cli`, `rustode-tui`, `rustode-gui`) depend on no rustode crate other than `rustode-workspace`,** whether as normal, dev or build dependencies. `xtask/tests/frontend_deps.rs` reads `cargo metadata` and fails otherwise (`:49-65`). It also checks itself against a fake dependency graph (`:67-79`). Third-party crates are not limited by this test.
2. The workspace re-exports what the frontends need, so that they never depend on the crate behind it. Examples: `rustode_workspace::trace` (`crates/workspace/src/lib.rs:16-19`), the core and fs types (`:14-15`), and `rustode_workspace::TokenKind`, through which the frontends color the highlights `Document::highlights` returns (ticket 27).
3. `rustode-core` does no I/O. Reading, writing, decoding and watching files live in `rustode-fs`.
4. A third-party crate stays behind the crate that owns its concern ([ADR 0010](0010-ecosystem-crates-allowed.md), rule 4).
5. Behavior studied in VS Code is credited per crate ([ADR 0009](0009-mit-and-reimplemented-behavior.md)). Paths are in the source tree; the full table, with each rustode file, is in [`ATTRIBUTION.md`](../../ATTRIBUTION.md).

| Crate | Behavior reimplemented | Studied in (Code - OSS, MIT) |
|---|---|---|
| core | undo grouping of typing | `src/vs/editor/common/cursor/cursorTypeEditOperations.ts:1011-1046`; tests in `src/vs/editor/test/browser/controller/cursor.test.ts:5925-6240` |
| core | box selection, add cursor above/below, add next occurrence | `src/vs/editor/common/cursor/cursorColumnSelection.ts:12-77`; `src/vs/editor/common/cursor/cursorMoveCommands.ts:18-46`; `src/vs/editor/contrib/multicursor/browser/multicursor.ts:355-410`, `:592-620` |
| fs | decoding order and lossy UTF-8 | `src/vs/workbench/services/textfile/common/encoding.ts:88-91` |
| fs, workspace | a deletion is believed after 100 ms; deleted files leave the document orphaned | `src/vs/workbench/services/textfile/common/textFileEditorModel.ts:158-201`, `:1063-1064` |
| workspace | a clean document reloads as one undoable edit; a dirty one never does | `src/vs/workbench/services/textfile/common/textFileEditorModelManager.ts:134-147`; `src/vs/editor/common/services/modelService.ts:365-387`; `src/vs/workbench/services/textfile/common/textFileEditorModel.ts:650` |
| workspace | conflicts settled by reverting or overwriting; the outdated saved text no longer counts as saved | `src/vs/workbench/contrib/files/browser/editors/textFileSaveErrorHandler.ts:113-135`, `:418-442`; `src/vs/workbench/services/textfile/common/textFileEditorModel.ts:404-406` |
| syntax | token colors: the TextMate scope VS Code's Rust grammar gives each kind of text, resolved as Dark Modern resolves it | `extensions/rust/syntaxes/rust.tmLanguage.json`; `extensions/theme-defaults/themes/dark_modern.json:4`, `:35`; `src/vs/workbench/services/themes/common/colorThemeData.ts:108-116`, `:358-375`, `:784-785`, `:807-810`, `:882-908` |
| syntax | highlight queries end with `(identifier)` as their least specific pattern | `src/vs/editor/common/languages/highlights/typescript.scm:1-5` |
| workspace | a file too large for tokenization when opened (20 MB or 300,000 lines) is never highlighted | `src/vs/editor/common/model/textModel.ts:189-190`, `:334-343` |
| config | JSON with comments and trailing commas, with error recovery | `src/vs/base/common/json.ts` |
| config | `when` clause grammar | `src/vs/platform/contextkey/common/scanner.ts`; `src/vs/platform/contextkey/common/contextkey.ts` |
| config | `keybindings.json` entries and key chords | `src/vs/workbench/services/keybinding/common/keybindingIO.ts`; `src/vs/base/common/keybindingParser.ts`; `src/vs/workbench/services/keybinding/browser/keybindingService.ts` |
| config | settings schema, unknown settings as hints, `[language]` overrides, reload | `src/vs/platform/configuration/common/configurationModels.ts`; `src/vs/platform/configuration/common/configurationRegistry.ts`; `src/vs/workbench/contrib/preferences/browser/preferencesRenderers.ts`; `src/vs/editor/common/config/editorConfigurationSchema.ts`; `src/vs/workbench/contrib/files/browser/files.contribution.ts` |

## The guarantee

A frontend can reach the buffer, the file layer or the configuration parser only through the workspace API, so each behavior is implemented once.

Check, from the project root: `cargo nextest run -p xtask -E 'binary(frontend_deps)'`. Both tests passed at `d6debfb`, measured on 2026-11-17 as part of the full suite (`cargo nextest run --workspace --locked --offline -j 4`: 133 passed, 2 skipped).

## Consequences

- Each crate builds and tests on its own, for example `cargo nextest run -p rustode-core`.
- Measured at `d6debfb`: the syntax, protocol, tui and gui crates are one-line placeholders (`wc -l crates/{tui,gui,protocol,syntax}/src/lib.rs`), and the CLI handles only `--version` and `--help` (`crates/cli/src/main.rs:5`).
- A new crate that keeps this boundary needs no new decision. A frontend that needs another rustode crate does.
