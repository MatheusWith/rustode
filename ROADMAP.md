# rustode: roadmap

**Study stage: preparation** · next: active analysis, once there is a bundle → source map of the main processes and the first component analyses ([why](docs/lacunas.md))

**Editor stage: Phase 1, M1 (core and CLI)** · next: the benchmark runner, the micro-benchmark gate and the CLI `open` command, then the TUI (M2) ([milestones](#editor-phase-1-milestones))

> **Last reviewed:** 2026-11-17 · **Last updated:** 2026-11-17 · **Change:** major (the editor's Phase 1 milestones, Gap Analysis, numbers and next steps added) · **Target:** local VSCodium 1.147.0 (VS Code `22d8dccf0f29`, VSCodium `ca7f7b44b5e5`, Electron 43.8.6) and the rustode editor at `d6debfb`

This page is the summary. The details are in:
- [`docs/qa.md`](docs/qa.md): tests;
- [`docs/lacunas.md`](docs/lacunas.md): open questions;
- [`docs/patches.md`](docs/patches.md): patches;
- [`09-analises/`](09-analises/): findings;
- [`docs/adr/`](docs/adr/): decisions, including the editor's (0007–0028).

## Key numbers

| | Value | How |
|---|---|---|
| App built and intact | yes, 3,008 files | measured: `sha256sum -c` against `02-engenharia-reversa/manifesto-sha256.txt`; re-checked on 2026-11-17 (silence, and the file list equals the manifest) |
| Patches applied | 55 (32 VSCodium unchanged, 19 adapted, 4 port-only); 6 skipped | measured: the `apply.sh` log (`06-port/logs/`) plus patch 96 |
| Test runs on the build | 15, no unexplained failure | measured: `08-testes-do-build/RESUMO.md` |
| Startup (workbench ready, p50) | 1905 ms (1.138: 2232 ms) | measured: `…-startup-bench`, 10 repetitions, virtual display |
| Hosts contacted unprompted | 4 groups, none from Microsoft | measured: `…-codium1.147-rede/network-report.md` |
| Build source tree | 14,465 files | measured: `find 02-engenharia-reversa/fonte/arvore -type f` |
| Recorded analyses | 2 | measured: `ls 09-analises/[0-9]*.md` |

Editor (rustode at `d6debfb`, measured on 2026-11-17 from the project root unless stated otherwise):

| | Value | How |
|---|---|---|
| Tests | 133 pass; 2 large stress cases skipped (100 MB, 1 GB) | measured: `cargo nextest run --workspace --locked --offline -j 4` |
| Line coverage | core crates 95.1% of 3,482 lines; frontends 85.7% of 21 lines (gates: 85% and 60%) | measured: `LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo xtask coverage` |
| Open 1 MB · 100 MB · 1 GB (release) | 2 ms · 151 ms · 2.29 s; peak RSS 5 · 218 · 2,208 MB | measured: the doc comments of `crates/workspace/tests/stress.rs:85`, `:93`, `:102-103`; command in `:4-6` |
| 10k random edits, then full undo, on 1 MB (release) | 9 ms; peak RSS 9 MB | measured: `crates/workspace/tests/stress.rs:136-137` |
| Third-party dependencies | 6 direct; 56 packages in the graph (all targets) | measured: `cargo metadata --format-version 1 --locked --offline` ([`ATTRIBUTION.md`](ATTRIBUTION.md)) |
| Licenses, advisories, bans, sources | all ok | measured: `cargo deny --locked --offline check` (local copy of the advisory database) |
| Decisions | 22 ADRs: 20 accepted, 2 proposed (0027, 0028) | counted: [`docs/adr/README.md`](docs/adr/README.md) |
| Comparison with the App | none yet; the first is planned for M3 | [ADR 0024](docs/adr/0024-comparison-with-the-app.md) |

## By dimension

| Dimension | State | Doc |
|---|---|---|
| Main process (`main.js`, `mainImpl.js`) | mapped by name; contents not studied | [architecture](docs/arquitetura.md) |
| Workbench (`workbench.desktop.main.js`) | mapped by name; not studied | [architecture](docs/arquitetura.md) |
| Sessions window (`sessions.desktop.main.js`) | mapped by name; not studied | [architecture](docs/arquitetura.md) |
| Extension host | mapped; integration tests pass | [qa](docs/qa.md) |
| Agent host | port patches studied (99-fixups); bundle not studied | [patches](docs/patches.md) |
| Packaging (asar, extensions, product.json) | compared with the source | [source-vs-package](docs/fonte-vs-pacote.md) |
| Privacy and network | measured | [0001](09-analises/0001-trafego-de-rede.md) |
| Extension compatibility | checked in the code; list of proposed APIs pending | [gaps G6](docs/lacunas.md) |
| Keystroke injection on the virtual display | measured: works through libei and `org_kde_kwin_fake_input` | [0002](09-analises/0002-entrada-virtual-wayland.md) |

## QA results

| Suite | Passed | Failed | Other | Last run | Evidence |
|---|---|---|---|---|---|
| Static smoke | 13 checks | 0 | — | 2026-11-16 | `smoke.sh` output |
| Network | 6 steps; verdict Passed | 0 | — | 2026-11-16 | `…-codium1.147-rede` |
| Menus | 11 | 0 | 1 not run (native About) | 2026-11-16 | `…-codium1.147-menus-r2` |
| Debugging / integrations | 9 / 10 | 0 | 1 blocked (account) | 2026-11-16 | `…-debug`, `…-integracoes` |
| Pending items (AI on) | 5 | 0 | — | 2026-11-16 | `…-pendencias-ia` |
| Recovery / offline | 3 / 4 | 0 | 1 caveat each | 2026-11-16 | `…-recuperacao`, `…-sem-rede` |
| Load | 5 | 1 (unsuitable criterion) | — | 2026-11-16 | `…-codium1.147-carga` |
| Stress | 4 | 0 | 2 caveats (RAM limit) | 2026-11-16 | `…-codium1.147-stress` |

## Editor: Phase 1 milestones

The scope of each milestone is fixed by [ADR 0018](docs/adr/0018-phase-1-milestones.md). Every feature milestone ends with a benchmark run and an update of the Gap Analysis below ([ADR 0024](docs/adr/0024-comparison-with-the-app.md)).

| Milestone | Scope | State | Evidence |
|---|---|---|---|
| M0, pipeline and governance | repository and privacy review; skeleton, pinned toolchain, lints; hooks; CI, Dependabot, cargo deny and audit; contributing guide and review checklist; the Editor section of `AGENTS.md` and the ADRs; the Wayland input spike | **in progress.** Done: repository, skeleton, hooks, CI and Dependabot files, spike. Waiting for the owner: the first push, then the branch-protection check (G19). In review: this documentation | commits `78aa4d5`, `569b179`, `9022c11`, `a780b8f`, `cd66bc1`, `7ee96c3`, `a33d58c`; `git status -sb` shows no upstream |
| M1, core and CLI | buffer, editing, multi-cursor, undo/redo, encoding and line-ending I/O, external changes, configuration, tracing; CLI `open`, `--version`, `bench` | **in progress.** Done: buffer, editing, multi-cursor, undo/redo, encoding and line-ending I/O, external changes, configuration, tracing, `--version`. Not started: the CLI's `open` and `bench`, and the micro-benchmark gate ([ADR 0026](docs/adr/0026-micro-benchmark-gate.md)) | commits `3c8fe23`, `c7bfd73`, `9d9313d`, `042e6fa`, `eacae33`, `1fbb01d`, `3e86468`, `46e0234`, `5e2b5ab`, `40cebd1`, `c10e816`, `d6debfb`; the CLI accepts only `--version` and `--help` (`crates/cli/src/main.rs:5`) |
| M2, workspace and TUI | workspace API; TUI with tabs, explorer, command palette, quick open, status bar, find/replace | not started | `crates/tui/src/lib.rs` is a one-line placeholder |
| M3, GUI | the same features in egui; the first cross-editor benchmark run | not started | `crates/gui/src/lib.rs` is a one-line placeholder |
| M4, syntax | Tree-sitter highlighting, language detection, Dark Modern token colors | not started | `crates/syntax/src/lib.rs` is a one-line placeholder |
| M5, LSP client | diagnostics, hover, completion, definition, references, rename, format; rust-analyzer and typescript-language-server | not started | — |
| M6, layout and search | global search with streaming and replace preview; split editors; minimap (GUI only) | not started | — |
| M7, DAP client | `launch.json`, breakpoints including conditional, run and step, call stack, variables, watch, debug console; lldb-dap and debugpy | not started | — |
| M8, extension management | Open VSX search, install, uninstall, enable, disable, list; nothing loaded; VS Code-format manifest and storage | not started | — |
| M9, Phase 1 report | full comparison with the App; complete Gap Analysis | not started | — |

## Gap Analysis against VS Code

What rustode does compared with the App (local VSCodium 1.147.0, built from VS Code `22d8dccf0f29`). Source-tree paths are relative to `02-engenharia-reversa/fonte/arvore/`. No performance comparison exists yet ([ADR 0024](docs/adr/0024-comparison-with-the-app.md)); this section compares behavior only.

### Implemented

| Feature | rustode | Differences from VS Code |
|---|---|---|
| Open and save keeping encoding and line endings | Decoding tries a BOM, then UTF-8, then Windows-1252, so nothing is lost. Round trips are byte-identical for UTF-8 with and without BOM, UTF-16 LE and BE with BOM (`crates/workspace/tests/open_save.rs:15-29`), Latin-1, LF, CRLF and mixed (`crates/workspace/tests/open_save.rs`; `c7bfd73`) | Deliberate: invalid UTF-8 falls back to Windows-1252, where VS Code decodes it with replacement characters (`src/vs/workbench/services/textfile/common/encoding.ts:88-91`). Missing: detecting UTF-16 without a BOM from the zero bytes in the first 512 bytes, flagging binary files (`seemsBinary`) (`encoding.ts:27`, `:477-530`), and guessing encodings with `files.autoGuessEncoding` (`encoding.ts:533`; `src/vs/workbench/contrib/files/browser/files.contribution.ts:206`) |
| Editing, undo and redo | Insert, delete and replace through the workspace. Typing groups into undo steps the way VS Code does. Undo back to the saved version leaves the document clean. A property test checks that any edit sequence followed by a full undo restores the text (`crates/workspace/tests/editing.rs:135-170`; `042e6fa`) | Missing: Enter handling and auto-indent (`src/vs/editor/common/cursor/cursorTypeEditOperations.ts:29`, `:573`); input-method composition (`:413`); `undoStopBefore` and `undoStopAfter` on programmatic edits (`src/vscode-dts/vscode.d.ts:1304-1312`) |
| Multi-cursor and box selection | Several selections, box selection that skips short lines, add cursor above or below, and `Ctrl+D` for the next occurrence; edits at every cursor undo as one step (`crates/workspace/tests/multi_cursor.rs`; `eacae33`) | Columns count characters, not visible columns, so a tab counts as one (`src/vs/editor/common/cursor/cursorColumnSelection.ts:25-26`). No sticky column when adding cursors across short lines (`src/vs/editor/common/cursor/cursorMoveOperations.ts:157-196`). `Ctrl+D` is a case-sensitive substring search; VS Code matches whole word and case from an empty selection, and otherwise follows `editor.selectedTextMatchMode` (`src/vs/editor/contrib/multicursor/browser/multicursor.ts:284-321`). Word characters are letters, digits and `_` (`crates/core/src/editor.rs:434`); VS Code uses `USUAL_WORD_SEPARATORS` (`src/vs/editor/common/core/wordHelper.ts:10`). `Ctrl+D` copies the whole text to search it (`crates/core/src/editor.rs:278`) |
| Files changed by other programs | A clean document reloads as one undo step. A dirty one raises a conflict, settled by reloading or keeping the edits. A deleted or renamed file leaves the document open and orphaned (`crates/workspace/tests/external_changes.rs`; `1fbb01d`, `3e86468`) | A save made before the next poll silently overwrites a newer file. VS Code sends the file's etag and turns a newer file into a conflict (`src/vs/workbench/services/textfile/common/textFileEditorModel.ts:1036`, `:1088-1090`); **owner decision**, see "Next steps". The conflict is raised by the watcher, not by the save, and there is no compare view (`src/vs/workbench/contrib/files/browser/editors/textFileSaveErrorHandler.ts:113-135`). Unsaved edits are not rebased onto the new file (`textFileEditorModel.ts:352-425`). The document does not follow a rename; it stays on its old path and reports where the file went (`crates/workspace/src/lib.rs:77-84`). Symlinks are not resolved (`grep -rn canonicalize crates` prints nothing). Documents cannot be closed, so their files are never unwatched (`crates/workspace/src/lib.rs` has no `close`). A reload builds two `Vec<char>` copies of the text (`crates/workspace/src/external.rs:207-208`). Tested on Linux (inotify) only |
| Settings and keybindings | `settings.json` and `keybindings.json` in VS Code's format, with comments and trailing commas. Unknown and invalid settings are reported with line and column. `when` clauses are parsed, `[language]` overrides are read, and a reload reports what changed (`crates/workspace/tests/config.rs`; `46e0234`, `5e2b5ab`) | The schema knows 9 settings (`crates/config/src/settings.rs:72-138`). None of them changes editing behavior yet: `grep -rn -e tabSize -e autoSave -e insertFinalNewline -e trimTrailing crates/core crates/fs crates/workspace/src crates/cli` prints nothing. `files.encoding` accepts any string (`settings.rs:79-80`). Deliberate: a keybinding whose `when` does not parse is dropped and reported; VS Code keeps it with no condition (`src/vs/workbench/services/keybinding/common/keybindingIO.ts:55-57`, `src/vs/platform/contextkey/common/contextkey.ts:205-233`) |
| Large files | 1 MB, 100 MB and 1 GB open within budget; 10k edits plus a full undo take 9 ms (see "Key numbers") | Not compared with the App yet (M3) |
| Tracing | One setup for every binary (`RUSTODE_LOG`, `RUSTODE_LOG_FORMAT`), span helpers, and an input-to-frame recorder with p50, p95 and p99 (`crates/trace/src/latency.rs:12`, `:62-69`; `3c8fe23`) | No frontend records frames yet |

### In progress

The open items of the milestones under way, M0 and M1:

- **M0:** the first push and the branch-protection check (owner); the editor documentation (this change, in review).
- **M1:** the benchmark runner (`rustode bench`) with the versioned scenario list; the criterion micro-benchmarks and their gate; the CLI `open` command.

### Backlog

| Milestone | Missing compared with VS Code |
|---|---|
| M2 | the TUI; editor groups and tabs; the explorer; the command registry, command palette (`Ctrl+Shift+P`) and quick open (`Ctrl+P`); the status bar; find and replace in a file; Dark Modern colors; the mouse; VS Code's default keybindings |
| M3 | the GUI with the same features; VS Code's width breakpoints; HiDPI; keyboard navigation and AccessKit |
| M4 | syntax highlighting (Tree-sitter, where VS Code uses TextMate grammars); language detection and a language picker |
| M5 | language servers: diagnostics, hover, completion, definition, references, rename, format |
| M6 | workspace search and replace across files; split editors; the minimap |
| M7 | debugging over DAP: `launch.json`, breakpoints, stepping, call stack, variables, watch, debug console |
| M8 | extension management against Open VSX (the Microsoft Marketplace is never used) |
| M9 | the full comparison with the App and the final Gap Analysis |

Out of scope for Phase 1 ([ADR 0018](docs/adr/0018-phase-1-milestones.md)): running extensions and loading their contributions; TextMate grammars; the Web frontend, MCP and remote; the integrated terminal; source control; js-debug and proprietary adapters and servers; Windows and macOS.

## Next steps

Study:

1. **Bundle → source map of the main processes** (G1). For each bundle in `out/`, list its entry points in `fonte/arvore/src` and the modules it includes. 6–10 h.
2. **2 h soak** (G4): the only measurement of leaks over time. 2 h of machine time + 1 h of analysis. **Owner decision:** authorize it and choose the time.
3. **Check the `product.json` list of proposed APIs** (G6) against the 1.147 proposals and VSCodium's newest list. 1–2 h. **Owner decision:** download the newest list from the network.
4. **Stress with more free RAM** (G3): open more than 75 files and test files of 10 to 60 MB. 1 h. **Owner decision:** close other apps or lower the soft limit.
5. **License of the analysis documents.** **Owner decision.**
6. **Index the source tree with CodeGraph.** This makes symbol search much faster, since the tree has about 10 thousand `.ts` files. **Owner decision** (indexing is always the owner's choice).
7. **Review the L05 criterion** (G10): baseline after a warm-up cycle. 30 min.

Editor:

1. **First push.** **Owner decision:** review the commits, then run `RUSTODE_ALLOW_MAIN_PUSH=1 git push -u origin main`; after that, check branch protection (G19).
2. **ADR 0028**, keystroke injection on the virtual display. **Owner decision:** accept it, so that typing latency can be measured ([ADR 0025](docs/adr/0025-typing-latency-injected-input.md)), or reject it.
3. **Saving over a newer file.** **Owner decision:** whether `save` should compare the disk stamp first and raise a conflict, as VS Code does with etags. The change would alter the open-and-save round-trip test, which overwrites the file before saving (`crates/workspace/tests/open_save.rs:48-54`).
4. **The benchmark runner** (`rustode bench`): the versioned scenario list, starting with "open a file", and the verified copy of the App (AGENTS.md §11.5).
5. **criterion micro-benchmarks and the regression gate** ([ADR 0026](docs/adr/0026-micro-benchmark-gate.md)).
6. **The CLI `open` command** (M1 scope).
7. **Before the TUI (M2).** **Owner decision:** the layout from 60 to 79 columns (G17), and the theme and the reuse of its colors (G18).
8. **The editor's `LICENSE` file.** **Owner decision:** its copyright line names the holder ([ADR 0009](docs/adr/0009-mit-and-reimplemented-behavior.md)).
9. **Integrity check that sees added files, and `NODE_DISABLE_COMPILE_CACHE=1` when launching the App from `01-app/`** (G13, proposed by finding 0002). **Owner decision.**

## Progress log

- **2026-11-17 (editor, documents):** the Editor section of `AGENTS.md` (§11); ADRs 0007–0028 for the Phase 1 decisions, with 0027 (ropey or a piece tree) proposed and 0028 (the native-input amendment) left as the owner's decision; the contributing guide's review checklist, mirrored by the pull request template; the editor's dependencies in `ATTRIBUTION.md`; this page's milestones and Gap Analysis; gaps G13–G19.
- **2026-11-17 (editor, M1):** editing with undo and redo grouped like VS Code (`042e6fa`); multi-cursor and box selection (`eacae33`); file watching and reload or conflict on external changes (`1fbb01d`, `3e86468`); settings and keybindings in VS Code's format (`46e0234`, `5e2b5ab`); stress budgets, with 1 GB measured at 2.29 s and a 2,208 MB peak (`40cebd1`, `c10e816`); UTF-8 decoded without copying (`9d9313d`; 100 MB peak from 318 MB to 218 MB); the workspace became the only rustode crate the frontends may use (`d6debfb`).
- **2026-11-17 (spike):** keystrokes reach the App and an eframe stand-in on the virtual display through libei and `org_kde_kwin_fake_input` ([0002](09-analises/0002-entrada-virtual-wayland.md), `a33d58c`). Its exploratory sessions wrote 3 compile-cache files into `01-app/`, which led to the rule against running the App in place (AGENTS.md §11.5; G13).
- **2026-11-17 (documents):** `AGENTS.md` §10 maps every artifact in the evidence folders (`16db07c`).
- **2026-11-16 (editor, M0 and M1):** the rustode folder became a git repository after the owner approved the privacy review (`78aa4d5`); the Cargo workspace with a working `--version` (`569b179`); commit and push hooks backed by xtask (`9022c11`, `a780b8f`); CI and Dependabot (`cd66bc1`); the license policy (`7ee96c3`); shared tracing (`3c8fe23`); open and save keeping encoding and line endings (`c7bfd73`).
- **2026-11-16 (language):** every project document translated to English (`AGENTS.md` now requires it). Folder and file names keep their Portuguese spelling, because `montar.sh` and the links depend on them.
- **2026-11-16 (tools):** REA 6.8.0 (`rea-agents`) registered as the project's MCP server for Claude Code (`.mcp.json`) and OpenCode (`opencode.json`), with the `reverse-engineer-anything` skill and the rules in `AGENTS.md` §6.
  - Verified: `rea mcp doctor` healthy (229 tools); the `codium` fuses were read; static analysis of `out/main.js` took 21 s, with no writes to 01 to 08.
  - Native decompilation is unavailable, because REA 6.8.0 only accepts Ghidra 12.1.x.
- **2026-11-16 (documents):** rustode gets the documentation set, following the filmcraft standard: `AGENTS.md` (single source; `CLAUDE.md` only imports it), `ROADMAP.md`, `docs/`, ADRs, templates and skills. First analysis recorded (network traffic).
- **2026-11-16 (rustode):** project assembled with real copies (no links) of the app, documents, screenshots, QA, port and reverse-engineering material; test results copied to `08-testes-do-build/`.
- **2026-11-16 (tests):** smoke 0 failures; network passed; CDP battery equal to 1.138 after fixing the harness (M12) and confirming VSCodium's AI default (P05); load and stress run.
- **2026-11-16 (build done):** commit recorded via `BUILD_SOURCEVERSION`; compile cache generated on a virtual display after patch 96 (UID suffix).
- **2026-11-16 (build, 3rd attempt):** bundle without source maps (patch 97) finishes in 65 s with a 2.7 GB peak; swap guard in `lib.sh`.
- **2026-11-16 (build, 1st and 2nd attempts):**
  - The parallel bundle hit a global OOM.
  - With the sequential bundle, the bundler's Node process sat at the memory cap for 30 min until the machine was rebooted.
- **2026-11-16 (port):** 54 patches applied and validated three ways: an import scan, a restricted typecheck, and reproducibility (`apply.sh` on a clean worktree reproduces the same tree).
- **2026-11-13 (QA):** QA of `code` 1.138 and of the 1.147 dev build: 75 features inventoried, findings P1–P12, startup of 2112 ms (1.138) against 4600 ms (dev).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-17 | major | Editor: Phase 1 milestones (M0–M9), Gap Analysis against VS Code, editor numbers and next steps; analysis 0002 |
| 2026-11-16 | minor | Translated to English |
| 2026-11-16 | minor | REA registered as a project tool (Claude Code and OpenCode) |
| 2026-11-16 | major | Created from the QA, the port and the build tests |
