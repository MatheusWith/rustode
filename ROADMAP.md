# rustode: roadmap

**Study stage: preparation** · next: active analysis, once there is a bundle → source map of the main processes and the first component analyses ([why](docs/lacunas.md))

**Editor stage: Phase 1, M1 (core and CLI) and M2 (workspace and TUI), both in progress** · next: the GUI merge, waiting on the font-license and advisory decisions (M3); the GUI tracer (M3); the CLI `open` command and the micro-benchmark gate (M1) ([milestones](#editor-phase-1-milestones))

> **Last reviewed:** 2026-11-17 · **Last updated:** 2026-11-17 · **Change:** minor (the editor's state brought up to `7c2467c`: the TUI, `rustode bench`, re-measured numbers, new Gap Analysis rows and the owner decisions the benchmark runner raised) · **Target:** local VSCodium 1.147.0 (VS Code `22d8dccf0f29`, VSCodium `ca7f7b44b5e5`, Electron 43.8.6) and the rustode editor at `7c2467c`

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

Editor (rustode at `7c2467c`, measured on 2026-11-17 from the project root unless stated otherwise):

| | Value | How |
|---|---|---|
| Tests | 225 pass; 2 large stress cases skipped (100 MB, 1 GB) | measured: `cargo nextest run --workspace --locked --offline -j 4`, debug build |
| Line coverage | core crates 95.5% of 3,869 lines; frontends 82.0% of 571 lines (gates: 85% and 60%) | measured at `a8c6252`, not re-measured since: `LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo xtask coverage` |
| Open 1 MB · 100 MB · 1 GB (release) | 2 ms · 151 ms · 2.29 s; peak RSS 5 · 218 · 2,208 MB | measured: the doc comments of `crates/workspace/tests/stress.rs:85`, `:93`, `:102-103`; command in `:4-6` |
| 10k random edits, then full undo, on 1 MB (release) | 9 ms; peak RSS 9 MB | measured: `crates/workspace/tests/stress.rs:136-137` |
| TUI key to frame | p50 0.71 ms, p99 0.88 ms over 600 keys (criterion: p99 under 16 ms, [ADR 0016](docs/adr/0016-tui-ratatui.md)) | measured at `a8c6252` (commit message): `cargo nextest run -p rustode-tui --no-capture`, debug build, ratatui's test backend at 120×40, test `key_to_frame_p99_stays_under_16_ms` (`crates/tui/tests/tui.rs:216`). The frame ends when `draw` returns; no terminal emulator is involved |
| Benchmark `open-file-1mb`, first run | rustode `Workspace::open` p50 0.776 ms (n = 20, in-process, no rendering); App launch to the file's first line shown p50 1946 ms (n = 3, cold launch from a verified copy). **Not like for like** until the GUI exists (M3); don't divide one by the other | measured on 2026-11-17 with `rustode bench open-file-1mb` (scenario list version 1, `bench/scenarios.toml`), before `ea68264`. 2 of its 4 App launches needed SIGKILL (`ea68264`). Its run folder was not kept in the repository. The rustode side alone, re-run at `7c2467c` (`cargo build --release -p rustode-cli`, then `target/release/rustode bench --side rustode --out <temporary folder> open-file-1mb`; release build, 1,085 MB available), gave p50 1.095 ms (n = 20) |
| Third-party dependencies | 11 direct; 139 packages in the graph (all targets) | measured: `cargo metadata --format-version 1 --locked --offline` ([`ATTRIBUTION.md`](ATTRIBUTION.md)) |
| Licenses, advisories, bans, sources | all ok; one warning: two versions of `syn` (2.0.119 and 3.0.10), since ratatui arrived at `a8c6252` | measured: `cargo deny --locked --offline check` (local copy of the advisory database); the same check at `865c8fa` printed no warning |
| Decisions | 22 ADRs: 20 accepted, 2 proposed (0027, 0028) | counted: [`docs/adr/README.md`](docs/adr/README.md) |
| Comparison with the App | the first benchmark run above, not like for like; the first like-for-like run is planned for M3 | [ADR 0024](docs/adr/0024-comparison-with-the-app.md) |

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
| M0, pipeline and governance | repository and privacy review; skeleton, pinned toolchain, lints; hooks; CI, cargo deny and audit (Dependabot was set up and removed at the owner's request on 2026-11-18); contributing guide and review checklist; the Editor section of `AGENTS.md` and the ADRs; the Wayland input spike | **in progress.** Done: repository, skeleton, hooks, CI files, spike, the editor documentation (the Editor section of `AGENTS.md`, ADRs 0007–0028, the review checklist). Waiting for the owner: the first push, then the branch-protection check (G19) | commits `78aa4d5`, `569b179`, `9022c11`, `a780b8f`, `cd66bc1`, `7ee96c3`, `a33d58c`, `2815ed9`, `19c4531`, `8b3b2d1`, `57e45cd`, `7c2467c`; `git status -sb` shows no upstream (checked on 2026-11-17) |
| M1, core and CLI | buffer, editing, multi-cursor, undo/redo, encoding and line-ending I/O, external changes, configuration, tracing; CLI `open`, `--version`, `bench` | **in progress.** Done: buffer, editing, multi-cursor, undo/redo, encoding and line-ending I/O, external changes, configuration, tracing, `--version`, and `bench`, the benchmark runner tracer with the versioned scenario list (one scenario, `open-file-1mb`). Not started: the CLI's `open`, and the micro-benchmark gate ([ADR 0026](docs/adr/0026-micro-benchmark-gate.md)) | commits `3c8fe23`, `c7bfd73`, `9d9313d`, `042e6fa`, `eacae33`, `1fbb01d`, `3e86468`, `46e0234`, `5e2b5ab`, `40cebd1`, `c10e816`, `d6debfb`; `bench`: `4203f14`, `176081c`, `ea68264`, `8a5ae5b`. The CLI accepts `--version`, `--help` and `bench` (`crates/cli/src/main.rs:7-9`), and `bench/scenarios.toml` lists the scenarios |
| M2, workspace and TUI | workspace API; TUI with tabs, explorer, command palette, quick open, status bar, find/replace | **in progress.** Done: the view API the frontends draw from (lines, cursor moves, Enter, Tab, Escape, select all, the status bar's line and column); the TUI tracer (open, edit and save, status bar, width bands, key-to-frame recorded, snapshot tests); tabs and the explorer; quick open with a fuzzy scorer, the command registry, the command palette and go to line; Dark Modern colors with 24-bit, 256, 16 or no colors. the mouse (click, drag, Shift and Alt clicks, wheel); the user's `keybindings.json` and settings applied live, with `when` clauses evaluated; word moves and deletes. find and replace in the current file (`744470c`, `7a2b1ca`, `706398e` engine; the TUI widget after them). M2's listed scope is complete | commits `a468316`, `a2c25d6`, `865c8fa` (view API); `a8c6252` (TUI tracer); `90ebf27`, `3081db1` (tabs, explorer); `22f0bbb`, `b813f94` (quick open, palette, go to line); `818073e` (colors); `4e2a0a2` (mouse); `44ca4a7`, `53dbb9d` (keybindings); `39ca85c` (word moves). Snapshots at each band in `crates/tui/tests/snapshots/`; test `each_width_band_has_its_layout` (`crates/tui/tests/tui.rs:90`) |
| M3, GUI | the same features in egui; the first cross-editor benchmark run | **in progress.** The GUI tracer is being built on a separate branch; nothing is merged | `crates/gui/src/lib.rs` on `main` is still a one-line placeholder |
| M4, syntax | Tree-sitter highlighting, language detection, Dark Modern token colors | not started | `crates/syntax/src/lib.rs` is a one-line placeholder |
| M5, LSP client | diagnostics, hover, completion, definition, references, rename, format; rust-analyzer and typescript-language-server | not started | — |
| M6, layout and search | global search with streaming and replace preview; split editors; minimap (GUI only) | not started | — |
| M7, DAP client | `launch.json`, breakpoints including conditional, run and step, call stack, variables, watch, debug console; lldb-dap and debugpy | not started | — |
| M8, extension management | Open VSX search, install, uninstall, enable, disable, list; nothing loaded; VS Code-format manifest and storage | not started | — |
| M9, Phase 1 report | full comparison with the App; complete Gap Analysis | not started | — |

## Gap Analysis against VS Code

What rustode does compared with the App (local VSCodium 1.147.0, built from VS Code `22d8dccf0f29`). Source-tree paths are relative to `02-engenharia-reversa/fonte/arvore/`. The first benchmark run is not like for like (see "Key numbers"), so no performance comparison exists yet ([ADR 0024](docs/adr/0024-comparison-with-the-app.md)); this section compares behavior only.

### Implemented

| Feature | rustode | Differences from VS Code |
|---|---|---|
| Open and save keeping encoding and line endings | Decoding tries a BOM, then UTF-8, then Windows-1252, so nothing is lost. Round trips are byte-identical for UTF-8 with and without BOM, UTF-16 LE and BE with BOM (`crates/workspace/tests/open_save.rs:15-29`), Latin-1, LF, CRLF and mixed (`crates/workspace/tests/open_save.rs`; `c7bfd73`) | Deliberate: invalid UTF-8 falls back to Windows-1252, where VS Code decodes it with replacement characters (`src/vs/workbench/services/textfile/common/encoding.ts:88-91`). Missing: detecting UTF-16 without a BOM from the zero bytes in the first 512 bytes, flagging binary files (`seemsBinary`) (`encoding.ts:27`, `:477-530`), and guessing encodings with `files.autoGuessEncoding` (`encoding.ts:533`; `src/vs/workbench/contrib/files/browser/files.contribution.ts:206`) |
| Editing, undo and redo | Insert, delete and replace through the workspace. Typing groups into undo steps the way VS Code does. Undo back to the saved version leaves the document clean. A property test checks that any edit sequence followed by a full undo restores the text (`crates/workspace/tests/editing.rs:135-170`; `042e6fa`). Enter inserts the document's line ending, keeps the line's indentation and starts an undo step that typing joins (`crates/workspace/src/lib.rs:456-467`; `a468316`) | Missing: auto-indent beyond keeping the line's indentation, from the language's indentation rules (`src/vs/editor/common/cursor/cursorTypeEditOperations.ts:29`, `:573`); input-method composition (`:413`); `undoStopBefore` and `undoStopAfter` on programmatic edits (`src/vscode-dts/vscode.d.ts:1304-1312`) |
| Multi-cursor and box selection | Several selections, box selection that skips short lines, add cursor above or below, and `Ctrl+D` for the next occurrence; edits at every cursor undo as one step (`crates/workspace/tests/multi_cursor.rs`; `eacae33`) | Columns count characters, not visible columns, so a tab counts as one (`src/vs/editor/common/cursor/cursorColumnSelection.ts:25-26`). No sticky column when adding cursors across short lines (`src/vs/editor/common/cursor/cursorMoveOperations.ts:157-196`). `Ctrl+D` is a case-sensitive substring search; VS Code matches whole word and case from an empty selection, and otherwise follows `editor.selectedTextMatchMode` (`src/vs/editor/contrib/multicursor/browser/multicursor.ts:284-321`). Word characters are letters, digits and `_` (`crates/core/src/editor.rs:444`); VS Code uses `USUAL_WORD_SEPARATORS` (`src/vs/editor/common/core/wordHelper.ts:10`). `Ctrl+D` copies the whole text to search it (`crates/core/src/editor.rs:286`) |
| Files changed by other programs | A clean document reloads as one undo step. A dirty one raises a conflict, settled by reloading or keeping the edits. A deleted or renamed file leaves the document open and orphaned (`crates/workspace/tests/external_changes.rs`; `1fbb01d`, `3e86468`) | A save made before the next poll silently overwrites a newer file. VS Code sends the file's etag and turns a newer file into a conflict (`src/vs/workbench/services/textfile/common/textFileEditorModel.ts:1036`, `:1088-1090`); **owner decision**, see "Next steps". The conflict is raised by the watcher, not by the save, and there is no compare view (`src/vs/workbench/contrib/files/browser/editors/textFileSaveErrorHandler.ts:113-135`). Unsaved edits are not rebased onto the new file (`textFileEditorModel.ts:352-425`). The document does not follow a rename; it stays on its old path and reports where the file went (`crates/workspace/src/lib.rs:90-97`). Symlinks are not resolved (`grep -rn canonicalize crates` prints nothing). Closing a document unwatches its file (`crates/workspace/src/lib.rs:226-240`; `90ebf27`). A reload builds two `Vec<char>` copies of the text (`crates/workspace/src/external.rs:207-208`). Tested on Linux (inotify) only |
| Settings and keybindings | `settings.json` and `keybindings.json` in VS Code's format, with comments and trailing commas. Unknown and invalid settings are reported with line and column. `when` clauses are parsed, `[language]` overrides are read, and a reload reports what changed (`crates/workspace/tests/config.rs`; `46e0234`, `5e2b5ab`) | The schema knows 9 settings (`crates/config/src/settings.rs:72-138`). Two change behavior: `editor.tabSize` and `editor.insertSpaces`, with their `[language]` overrides, drive Tab and the status bar's column (`crates/workspace/src/lib.rs:469-504`, `:513-530`; `a2c25d6`). The other seven don't yet: `grep -rn -e detectIndentation -e files.eol -e autoSave -e insertFinalNewline -e trimTrailing crates/core crates/fs crates/workspace/src crates/cli crates/tui/src` prints nothing, and `files.encoding` appears only in a doc comment (`crates/fs/src/encoding.rs:19`). `files.encoding` accepts any string (`settings.rs:79-80`). Deliberate: a keybinding whose `when` does not parse is dropped and reported; VS Code keeps it with no condition (`src/vs/workbench/services/keybinding/common/keybindingIO.ts:55-57`, `src/vs/platform/contextkey/common/contextkey.ts:205-233`) |
| Large files | 1 MB, 100 MB and 1 GB open within budget; 10k edits plus a full undo take 9 ms (see "Key numbers") | Not compared with the App yet (M3) |
| Tracing | One setup for every binary (`RUSTODE_LOG`, `RUSTODE_LOG_FORMAT`), span helpers, and an input-to-frame recorder with p50, p95 and p99 (`crates/trace/src/latency.rs:12`, `:62-69`; `3c8fe23`). The TUI records key-to-frame latency for every key and logs the report at exit (`crates/tui/src/app.rs:82`, `crates/tui/src/main.rs:35-36`; `a8c6252`) | The TUI's figure is measured on ratatui's test backend only; a measurement through a terminal emulator needs the virtual display. The GUI records nothing yet (M3) |
| Line breaks | Only CR, LF and CRLF end a line; U+2028, U+2029 and U+0085 stay inside it (ropey with default features off, `cr_lines` and `simd` on, `Cargo.toml`; test `only_cr_lf_and_crlf_break_lines_as_in_vs_code`, `crates/workspace/tests/view.rs:49`; `a468316`). Inserted breaks use the document's line ending | Same rule as VS Code's text buffer (`src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeBase.ts:37-62`) |
| TUI: layout and status bar | `rustode-tui PATH...` opens files in tabs and the first folder in the explorer (`crates/tui/src/main.rs:1-2`). Width bands: sidebar at 120 columns or more, or while the explorer has focus; status bar from 60 columns; editor only below 60, where a focused explorer takes the whole width; a "terminal too small" notice below 40×10 (`crates/tui/src/view.rs:32-68`; snapshots in `crates/tui/tests/snapshots/`). The status bar shows `Ln, Col`, the encoding, the line ending and the language (`crates/tui/src/view.rs:300`). Control and bidirectional formatting characters are drawn visibly, as VS Code does (`ATTRIBUTION.md`; `a8c6252`) | From 60 to 79 columns the TUI shows what it shows from 80 to 119: status bar, no sidebar. That is rustode's choice, pending the owner (G17). Missing: the current-line highlight (`editor.renderLineHighlight`, `src/vs/editor/common/config/editorOptions.ts:6739`); "(n selected)" after the position (`src/vs/workbench/browser/parts/editor/editorStatus.ts:338-340`); the indentation indicator, such as "Spaces: 4" (`editorStatus.ts:803-806`) |
| TUI: editing keys | Typing, Enter, Tab, Backspace and Delete, arrows, Home and End, Page Up and Down, with Shift to select; undo, redo, select all, Escape, `Ctrl+D`, add cursor above or below; save on `Ctrl+S`, quit on `Ctrl+Q`; bracketed paste (`crates/tui/src/keys.rs:69-88`, `:124-199`; `crates/tui/src/main.rs:31`; `a8c6252`) | Word moves and deletes follow `cursorWordLeft`/`cursorWordEndRight`/`deleteWordLeft`/`deleteWordRight` (`39ca85c`) without CJK segmentation; mouse support is in `4e2a0a2`. Missing: input-method composition (`src/vs/editor/common/cursor/cursorTypeEditOperations.ts:413`); `editor.detectIndentation` (`src/vs/editor/common/config/editorConfigurationSchema.ts:51`), so Tab always follows the settings. The user's `keybindings.json` is applied (`53dbb9d`): single-chord bindings only (chord sequences such as `ctrl+k ctrl+c` are not), and `when` clauses with `=~` or `in` evaluate as false (`crates/config/src/when.rs`) |
| TUI: tabs and explorer | One tab per file; `Ctrl+PageDown`/`PageUp` switch with wrap-around, `Ctrl+W` closes and activates the most recently used tab. The explorer lists folders first, then names with numbers compared as numbers, case ignored first, and hides `files.exclude`'s defaults; Left, Right, Enter and Space as in VS Code (`crates/workspace/tests/tabs.rs:109`; `90ebf27`, `3081db1`) | `Ctrl+Shift+E` needs a terminal that reports Shift together with Ctrl. The TUI does not request the kitty keyboard protocol (`grep -rn KeyboardEnhancement crates/tui/src` prints nothing), and in the legacy encoding the key arrives as `Ctrl+E`, so F6, which moves focus to the next part, is offered and works everywhere (`crates/tui/src/keys.rs:72`, `:77`). VS Code compares names with the locale's ICU collation (`src/vs/base/common/comparers.ts:11-16`); rustode approximates it (`crates/workspace/src/explorer.rs:4-9`). The user's `files.exclude` patterns are not read (`crates/workspace/src/explorer.rs:10-13`) |
| Quick open, command palette, go to line | `Ctrl+P` ranks the folder's files with a reimplementation of VS Code's fuzzy scorer and honors `.gitignore`, `.ignore` and the `files.exclude` and `search.exclude` defaults; with an empty query it lists the open editors (`crates/workspace/tests/quick_open.rs:62`). `Ctrl+Shift+P` or F1 lists commands with VS Code's ids, labels and default Linux keybindings; `:` and `Ctrl+G` go to a line (`22f0bbb`, `b813f94`) | The palette finds each typed word as a substring of the label and lists matches alphabetically (`crates/workspace/src/commands.rs:170-185`, `:13-15`). VS Code's filter also matches word starts (`matchesWords`, `src/vs/platform/quickinput/browser/commandsQuickAccess.ts:56`) and puts recently used commands first |
| TUI colors | Dark Modern's workbench colors, with the color registry's defaults where the theme sets none, in 24-bit color, the xterm 256-color palette or 16 colors, chosen from `COLORTERM` and `TERM`. `NO_COLOR` turns colors off and leaves bold, dim and reversed; `RUSTODE_COLOR` overrides the choice (`crates/tui/src/theme.rs:11-54`; snapshots `colors-*-70x10.txt`; `818073e`) | The App's default dark theme is "Dark 2026", not Dark Modern (`src/vs/workbench/services/themes/common/workbenchThemeService.ts:43`); whether to follow it, and whether the color values may be reused, is the owner's (G18). 256 and 16 colors approximate the theme's values. No syntax colors yet (M4) |

### In progress

The open items of the milestones under way, M0 to M3:

- **M0:** the first push and the branch-protection check (owner).
- **M1:** the criterion micro-benchmarks and their gate ([ADR 0026](docs/adr/0026-micro-benchmark-gate.md)); the CLI `open` command. Not started.
- **M3:** the GUI tracer, being built on a separate branch.

### Backlog

| Milestone | Missing compared with VS Code |
|---|---|
| M2 | find and replace in a file; keybinding chord sequences; the TUI gaps listed under "Implemented" |
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
4. **What the benchmark runner's first run raised** (`rustode bench`, `4203f14`, `176081c`, `ea68264`, `8a5ae5b`). **Owner decisions:**
   1. *The one-App guard.* It refuses while any `codium` process runs, including the owner's own VSCodium: it matches any executable or first argument named `codium` or under a `VSCode-linux-x64` folder (`crates/cli/src/bench/guard.rs:82-133`). Keep it, or narrow it to the App's copies.
   2. *The wait before shutdown.* The driver takes its screenshot and sends SIGTERM as soon as the first line shows (`bench/app/open_file.cjs:25`, `:124-135`); that was about 0.5 s after the first line in the first run, as reported, since its run folder was not kept. The QA startup benchmark waits 2.5 s first (`05-qa/suite/scenarios/startup_bench.cjs:43`). In the first run, 2 of the 4 App launches needed SIGKILL after 10 s (`ea68264`). Adding the wait needs one more App run.
   3. *Record the first run* as an analysis in `09-analises/`? Its run folder was not kept, so recording it means running it again.
   4. *The App's launch flags*, copied from the QA startup benchmark: an isolated profile, `--disable-telemetry`, `--disable-workspace-trust`, CDP and `--new-window`, under a 3 GB memory cap (`crates/cli/src/bench/plan.rs:17-27`, `:199-215`; `05-qa/suite/scenarios/startup_bench.cjs:31-33`). The runner adds `MemorySwapMax=1G`, which the startup benchmark does not set. Confirm them.
   5. *The duplicate `syn`* that `cargo deny` warns about. It came with ratatui at `a8c6252` (the check at `865c8fa` prints no warning): ratatui's proc-macro dependencies use both syn 2.0.119 and 3.0.10. `serde_derive`, which `rustode bench` added for its scenario list, uses syn 3.0.10, which was already there, so parsing the TOML by hand would not remove the warning (measured: `cargo tree --locked --offline -i syn@2.0.119` and `-i syn@3.0.10` at `7c2467c`). Accept the warning, or keep looking for a way to remove it.
5. **criterion micro-benchmarks and the regression gate** ([ADR 0026](docs/adr/0026-micro-benchmark-gate.md)).
6. **The CLI `open` command** (M1 scope).
7. **Confirm the TUI's layout and theme** (G17, G18). **Owner decision.** The TUI already made a choice for each, which stands until the owner confirms or changes it:
   - from 60 to 79 columns it shows what it shows from 80 to 119: the status bar and no sidebar (`crates/tui/src/view.rs:32-68`);
   - its theme is Dark Modern, as the Phase 1 specification asks (story 70) and [ADR 0016](docs/adr/0016-tui-ratatui.md) records, while the App's default dark theme is "Dark 2026" (`src/vs/workbench/services/themes/common/workbenchThemeService.ts:43`, in the source tree). Its color values are taken from `extensions/theme-defaults/themes/dark_modern.json` and the color registry (`crates/tui/src/theme.rs`; `ATTRIBUTION.md`).
8. **The editor's `LICENSE` file.** **Owner decision:** its copyright line names the holder ([ADR 0009](docs/adr/0009-mit-and-reimplemented-behavior.md)).
9. **Integrity check that sees added files, and `NODE_DISABLE_COMPILE_CACHE=1` when launching the App from `01-app/`** (G13, proposed by finding 0002). **Owner decision.** `rustode bench` already fails a run when `01-app/` holds a file the manifest does not list, before and after the run (`crates/cli/src/bench.rs:5-6`, `crates/cli/src/bench/integrity.rs:20-54`; commit message of `176081c`).

## Progress log

- **2026-11-17 (editor, M1 and M2):** the view API that frontends draw from, with Enter keeping indentation, Tab following `editor.tabSize` and `editor.insertSpaces`, and only CR, LF and CRLF breaking lines (`a468316`, `a2c25d6`, `865c8fa`); the TUI tracer, with the status bar, the width bands, key-to-frame latency recorded and snapshot tests (`a8c6252`); tabs, the explorer and closing a document, which unwatches its file (`90ebf27`, `3081db1`); quick open with a fuzzy scorer, the command registry, the command palette and go to line (`22f0bbb`, `b813f94`); Dark Modern colors with 24-bit, 256, 16 or no colors (`818073e`); `rustode bench`, the benchmark runner tracer, with its first real run of `open-file-1mb` (`4203f14`, `176081c`, `ea68264`, `8a5ae5b`). This page, the README, ADR 0016, `AGENTS.md` §11.1, `ATTRIBUTION.md` and gaps G17–G18 brought up to `7c2467c`.
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
- **2026-11-16 (rustode):** project assembled with real copies (no links) of the app, documents, screenshots, QA, port and analysis material; test results copied to `08-testes-do-build/`.
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
| 2026-11-17 | minor | Editor state at `7c2467c`: the TUI (M2, in part), `rustode bench`, re-measured numbers, TUI rows in the Gap Analysis, the owner decisions the benchmark runner raised |
| 2026-11-17 | major | Editor: Phase 1 milestones (M0–M9), Gap Analysis against VS Code, editor numbers and next steps; analysis 0002 |
| 2026-11-16 | minor | Translated to English |
| 2026-11-16 | minor | REA registered as a project tool (Claude Code and OpenCode) |
| 2026-11-16 | major | Created from the QA, the port and the build tests |
