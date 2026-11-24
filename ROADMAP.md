# rustode: roadmap

**Study stage: preparation** · next: active analysis, once there is a bundle → source map of the main processes and the first component analyses ([why](docs/gaps.md))

**Editor stage: Phase 1 complete (M0 to M9 merged); next: Phase 2** · the owner's open decisions and the Phase 2 backlog are in [finding 0005](09-findings/0005-phase-1-report.md) ([milestones](#editor-phase-1-milestones), [Next steps](#next-steps))

> **Last reviewed:** 2026-11-23 · **Last updated:** 2026-11-23 · **Change:** major (Phase 1 report: milestones M0–M9, Gap Analysis, key numbers and Phase 2 starting point) · **Target:** local VSCodium 1.147.0 (VS Code `22d8dccf0f29`, VSCodium `ca7f7b44b5e5`, Electron 43.8.6) and the rustode editor at `8c3c9ab`

This page is the summary. The details are in:
- [`docs/qa.md`](docs/qa.md): tests;
- [`docs/gaps.md`](docs/gaps.md): open questions;
- [`docs/patches.md`](docs/patches.md): patches;
- [`09-findings/`](09-findings/): findings;
- [`docs/adr/`](docs/adr/): decisions, including the editor's (0007–0031).

## Key numbers

| | Value | How |
|---|---|---|
| App built and intact | yes, 3,008 files | measured: `sha256sum -c` against `02-package-analysis/manifest-sha256.txt`; re-checked on 2026-11-17 (silence, and the file list equals the manifest) |
| Patches applied | 55 (32 VSCodium unchanged, 19 adapted, 4 port-only); 6 skipped | measured: the `apply.sh` log (`06-port/logs/`) plus patch 96 |
| Test runs on the build | 15, no unexplained failure | measured: `08-build-tests/SUMMARY.md` |
| Startup (workbench ready, p50) | 1905 ms (1.138: 2232 ms) | measured: `…-startup-bench`, 10 repetitions, virtual display |
| Hosts contacted unprompted | 4 groups, none from Microsoft | measured: `…-codium1.147-rede/network-report.md` |
| Build source tree | 14,465 files | measured: `find 02-package-analysis/source/tree -type f` |
| Recorded analyses | 5 | measured: `ls 09-findings/[0-9]*.md` |

Editor (rustode at `8c3c9ab`, measured on 2026-11-23 from the project root unless stated otherwise):

| | Value | How |
|---|---|---|
| Tests | 883 pass; 17 skipped | measured, debug build, rustc 1.99.1: `cargo nextest run --workspace --locked` |
| Rust source | 77,218 lines in `crates/` and `xtask/` | measured: `find crates xtask -name "*.rs" \| xargs cat \| wc -l` |
| Line coverage | core crates 94.3% of 16,486 lines; frontends 91.0% of 14,345 lines (gates: 85% and 60%) | measured: `LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo xtask coverage` |
| Open 1 MB · 100 MB · 1 GB (release) | 2 ms · 151 ms · 2.29 s; peak RSS 5 · 218 · 2,208 MB | measured: the doc comments of `crates/workspace/tests/stress.rs:85`, `:93`, `:102-103`; command in `:4-6` |
| 10k random edits, then full undo, on 1 MB (release) | 9 ms; peak RSS 9 MB | measured: `crates/workspace/tests/stress.rs:136-137` |
| TUI key to frame | p50 0.71 ms, p99 0.88 ms over 600 keys (criterion: p99 under 16 ms, [ADR 0016](docs/adr/0016-tui-ratatui.md)) | measured at `a8c6252` (commit message): `cargo nextest run -p rustode-tui --no-capture`, debug build, ratatui's test backend at 120×40, test `key_to_frame_p99_stays_under_16_ms` (`crates/tui/tests/tui.rs:216`). The frame ends when `draw` returns; no terminal emulator is involved |
| Third-party dependencies | 33 direct (normal) dependencies of the 12 crates; 546 packages in the graph (all targets and kinds) | measured: `cargo metadata --format-version 1 --locked --offline` ([`ATTRIBUTION.md`](ATTRIBUTION.md)) |
| Licenses, advisories, bans, sources | all ok; 25 warnings about crates present in two versions (for example `bitflags`, `hashbrown`, `itertools`), which the GUI's dependencies bring | measured, 2026-11-23: `cargo deny --locked check`, counted with `grep -cE "^warning\[duplicate\]"` |
| Decisions | 31 ADRs: 6 for the study, 25 for the editor (22 accepted, 3 proposed: 0027, 0028, 0029) | counted: [`docs/adr/README.md`](docs/adr/README.md) |
| Full suite against the App (M9), startup to first frame, p50 (n = 20 per side) | warm file cache: rustode 154 ms (p95 169 ms); App first swap 1,939 ms (p95 2,064 ms), first paint 1,835 ms, first line shown 1,928 ms. Cold file cache: rustode 179 ms (p95 334 ms); App first swap 2,184 ms. The App's first swap, the end of the first `Display::DrawAndSwap` after the first line entered the DOM, is the counterpart of rustode's first present: 12.6 times warm and 12.2 times cold (computed); it may be one frame early and carries the trace's cost, about 37 ms (one control pair) | measured on 2026-11-23, `rustode bench <scenario>` (list version 3) at `6d7b04e50f98` (`open-file-1gb` at `8ee6e839b64e`, runner change only), release `rustode-gui`, both editors in software rendering (llvmpipe) on `kwin_wayland --virtual`; [`09-findings/0004`](09-findings/0004-full-app-suite.md) has the commands, definitions, every percentile and the limits |
| Full suite, idle memory 20 s after ready, p50 (n = 5) | rustode: 1 process, 132 MB `Rss`, 103 MB `Pss`; App: 1,605 MB `Rss`, 853 MB `Pss` (the App also runs built-in extensions, a terminal and source control that rustode lacks) | measured on 2026-11-23, `rustode bench <scenario>` (list version 3) at `6d7b04e50f98` (`open-file-1gb` at `8ee6e839b64e`, runner change only), release `rustode-gui`, both editors in software rendering (llvmpipe) on `kwin_wayland --virtual`; [`09-findings/0004`](09-findings/0004-full-app-suite.md) has the commands, definitions, every percentile and the limits |
| Full suite, opening 1 MB · 100 MB · 1 GB, p50 | rustode launch to first frame 151 ms · 322 ms · 2.34 s (n = 5, 5, 3); App launch to first swap 1,931 ms · 2,390 ms · 6.36 s. Memory with the file open (`Rss`/`Pss`): 134/108 · 246/220 · 1,302/1,294 MB against 1,362/622 · 1,512/773 · 2,314/1,726 MB. In-process `Workspace::open`: 1.2 ms · 129 ms (n = 20, 10); the 1 GiB one was skipped for lack of free memory in this set (finding 0003: 1.64 s); no window, not comparable with the App | measured on 2026-11-23, `rustode bench <scenario>` (list version 3) at `6d7b04e50f98` (`open-file-1gb` at `8ee6e839b64e`, runner change only), release `rustode-gui`, both editors in software rendering (llvmpipe) on `kwin_wayland --virtual`; [`09-findings/0004`](09-findings/0004-full-app-suite.md) has the commands, definitions, every percentile and the limits |
| Full suite, 10k random inserts then undo of all, on 1 MiB, p50 (n = 5) | rustode: inserts 13.8 ms, to next frame 23.5 ms; undo 10.8 ms, to next frame 17.9 ms; peak RSS after 144 MB. App: inserts 9,338 ms, to next frame 9,490 ms; undo 9,774 ms, to next frame 10,075 ms; peak RSS after 2,246 MB (sum over processes). Same seeded edits, same text after the inserts and after the undo (hash). **Not like for like**: the App's model API takes line and column and fires its editor's listeners on every insert (G25) | measured on 2026-11-23, `rustode bench <scenario>` (list version 3) at `6d7b04e50f98` (`open-file-1gb` at `8ee6e839b64e`, runner change only), release `rustode-gui`, both editors in software rendering (llvmpipe) on `kwin_wayland --virtual`; [`09-findings/0004`](09-findings/0004-full-app-suite.md) has the commands, definitions, every percentile and the limits |
| Full suite, regex search over a generated monorepo (5,000 source files, 91.8 MB), p50 (n = 5) | rustode: first result 96.7 ms, complete 96.7 ms; App: first result 420 ms, complete 532 ms. Both found 10,436 matches in 3,002 files, which the generator's oracle and `grep` confirm. Two engines and two views; warm cache (G26) | measured on 2026-11-23, `rustode bench <scenario>` (list version 3) at `6d7b04e50f98` (`open-file-1gb` at `8ee6e839b64e`, runner change only), release `rustode-gui`, both editors in software rendering (llvmpipe) on `kwin_wayland --virtual`; [`09-findings/0004`](09-findings/0004-full-app-suite.md) has the commands, definitions, every percentile and the limits |
| Typing latency | **not measured: waiting on [ADR 0028](docs/adr/0028-virtual-display-input.md)** | AGENTS.md §11.5.5 |
| Comparison with the App | the whole versioned list ran on 2026-11-23 (M9 suite, above): every scenario of [ADR 0024](docs/adr/0024-comparison-with-the-app.md) but typing latency, which waits for ADR 0028. The App's first presented frame comes from a Chromium trace, so startup has a counterpart event for rustode's first frame (G22). The next comparison is planned for M4's end | [ADR 0024](docs/adr/0024-comparison-with-the-app.md), [finding 0004](09-findings/0004-full-app-suite.md), [finding 0003](09-findings/0003-first-app-comparison.md) |

## By dimension

| Dimension | State | Doc |
|---|---|---|
| Main process (`main.js`, `mainImpl.js`) | mapped by name; contents not studied | [architecture](docs/architecture.md) |
| Workbench (`workbench.desktop.main.js`) | mapped by name; not studied | [architecture](docs/architecture.md) |
| Sessions window (`sessions.desktop.main.js`) | mapped by name; not studied | [architecture](docs/architecture.md) |
| Extension host | mapped; integration tests pass | [qa](docs/qa.md) |
| Agent host | port patches studied (99-fixups); bundle not studied | [patches](docs/patches.md) |
| Packaging (asar, extensions, product.json) | compared with the source | [source-vs-package](docs/source-vs-package.md) |
| Privacy and network | measured | [0001](09-findings/0001-network-traffic.md) |
| Extension compatibility | checked in the code; list of proposed APIs pending | [gaps G6](docs/gaps.md) |
| Keystroke injection on the virtual display | measured: works through libei and `org_kde_kwin_fake_input` | [0002](09-findings/0002-virtual-wayland-input.md) |

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

The scope of each milestone is fixed by [ADR 0018](docs/adr/0018-phase-1-milestones.md). Every feature milestone ends with a benchmark run and an update of the Gap Analysis below ([ADR 0024](docs/adr/0024-comparison-with-the-app.md)). State at `8c3c9ab`; the per-milestone detail, with every merged pull request, is in [finding 0005](09-findings/0005-phase-1-report.md).

| Milestone | Scope | State | Evidence |
|---|---|---|---|
| M0, pipeline and governance | repository and privacy review; skeleton, pinned toolchain, lints; hooks; CI, cargo deny and audit; contributing guide and review checklist; the Editor section of `AGENTS.md` and the ADRs; the Wayland input spike | **done.** The repository is on GitHub, `main` takes squash-merged pull requests only, and branch protection exists (measured; its list of required checks is empty) | commits `78aa4d5`, `569b179`, `9022c11`, `a780b8f`, `cd66bc1`, `7ee96c3`, `2815ed9`, `19c4531`, `8b3b2d1`, `57e45cd`, `a33d58c`; #1, #2, #4, #7, #17 to #22 |
| M1, core and CLI | buffer, editing, multi-cursor, undo/redo, encoding and line-ending I/O, external changes, configuration, tracing; CLI `open`, `--version`, `bench` | **done except the CLI `open` command** (`crates/cli/src/main.rs:8-25`); the micro-benchmarks and their gate are applied (#11, [ADR 0026](docs/adr/0026-micro-benchmark-gate.md)) | commits `3c8fe23`, `c7bfd73`, `042e6fa`, `eacae33`, `9d9313d`, `40cebd1`, `1fbb01d`, `3e86468`, `46e0234`, `5e2b5ab`, `d6debfb`, `39ca85c`, `cf51f67`, `44ca4a7`; #11 |
| M2, workspace and TUI | workspace API; TUI with tabs, explorer, command palette, quick open, status bar, find/replace | **done**, with the gaps in finding 0005 (chord sequences in user keybindings, TUI layout from 60 to 79 columns, G17) | `a8c6252`, `90ebf27`, `3081db1`, `22f0bbb`, `b813f94`, `818073e`, `4e2a0a2`, `53dbb9d`, `706398e`, `7a2b1ca`, `8a6fe63` |
| M3, GUI | the same features in egui; the first cross-editor benchmark run | **done**: tabs, explorer, status bar, palette, quick input, find, breakpoints, HiDPI, keyboard focus, AccessKit; first comparison with the App ([finding 0003](09-findings/0003-first-app-comparison.md)). Missing: clipboard, drag-to-resize | #3, #8, #14, #16, #36 |
| M4, syntax | Tree-sitter highlighting, language detection, Dark Modern token colors | **done**: 14 grammars, incremental reparse (`crates/syntax/Cargo.toml:13-27`) | #12, #23 |
| M5, LSP client | diagnostics, hover, completion, definition, references, rename, format; rust-analyzer and typescript-language-server | **done**; servers start by default and TypeScript 6 is handled | #10, #24, #28, #29, #33 |
| M6, layout and search | global search with streaming and replace preview; split editors; minimap (GUI only) | **done**; G20, G21 list what differs | #15, #27, #30, #32, #35 |
| M7, DAP client | `launch.json`, breakpoints including conditional, run and step, call stack, variables, watch, debug console; lldb-dap and debugpy | **done** with the tracer's gaps (no attach, no `runInTerminal`) | #26, #31, #34 |
| M8, extension management | Open VSX search, install, uninstall, enable, disable, list; nothing loaded; VS Code-format manifest and storage | **done**; [ADR 0029](docs/adr/0029-extension-management-open-vsx.md) is still proposed | #9, #13 |
| M9, Phase 1 report | full comparison with the App; complete Gap Analysis | **done** with this report; typing latency is not measured (ADR 0028) | #36, #38 ([finding 0004](09-findings/0004-full-app-suite.md)), [finding 0005](09-findings/0005-phase-1-report.md) |

## Gap Analysis against VS Code

What rustode does compared with the App (local VSCodium 1.147.0, built from VS Code `22d8dccf0f29`). The full analysis, with the evidence of every row, is [finding 0005](09-findings/0005-phase-1-report.md); this section is its summary. Source-tree paths are relative to `02-package-analysis/source/tree/`.

| Part | Result | Largest gaps |
|---|---|---|
| Features (36 rows) | 6 implemented, 22 partial, 4 missing, 4 out of scope for Phase 1 | clipboard in the GUI and copy/cut everywhere; file operations in the explorer; line commands (comment, move, delete, indent); code actions, signature help, symbols; folding, bracket matching, word wrap; 7 of the 21 known settings do nothing; saving over a newer file; hot exit and backups |
| Shortcuts (71 commands) | 53 have a default key, all 53 equal VS Code's Linux key (0 different); 5 VS Code secondary keys are not registered; about 25 notable VS Code defaults have no command | Ctrl+C/X/V, Ctrl+N, Ctrl+/, Alt+Up/Down, Ctrl+Shift+K, Ctrl+., F8, Ctrl+Shift+O; the TUI cannot receive keys that need the kitty keyboard protocol (Ctrl+Shift+E, Shift+F12, Ctrl+Shift+I, Ctrl+Shift+1, Ctrl+1 to 3, Ctrl+Alt+arrows) |
| Extension compatibility | management only: Open VSX search, install, uninstall, enable, disable in VS Code's storage format; nothing is loaded | no extension host (out of scope for Phase 1), no signature verification, no updates ([ADR 0029](docs/adr/0029-extension-management-open-vsx.md), proposed) |
| UI | TUI: 5 of 5 criteria met on the test backend; GUI: 5 of 6 met in tests (fractional scaling and Orca only by test, not on a real display), 1 not measured | input-to-photon p99 not measured (ADR 0028); Orca end to end and fractional scaling not checked; G17, G18 |

Out of scope for Phase 1 ([ADR 0018](docs/adr/0018-phase-1-milestones.md)): running extensions and loading their contributions; TextMate grammars; the Web frontend, MCP and remote; the integrated terminal; source control; js-debug and proprietary adapters and servers; Windows and macOS.

### Performance against the App

The whole versioned list, 2026-11-23, from [finding 0004](09-findings/0004-full-app-suite.md) (it re-ran finding 0003's scenarios and added the 10k-edit and search scenarios): both editors on the same `kwin_wayland --virtual` display in software rendering, the App from a verified copy, one at a time, release `rustode-gui`, at `6d7b04e50f98`. Medians; the finding has n, p95 and the definitions. Absolute times do not hold for a real GPU session ([ADR 0006](docs/adr/0006-tests-on-virtual-display.md)). "Launch to first swap" is the App's first presented frame after the first line entered the DOM, read from a Chromium trace (G22); ratios use it, not the first-paint or first-line numbers.

| Scenario | rustode | App | What the App has that rustode lacks, or the caveat |
|---|---|---|---|
| Startup to first frame, warm cache | 154 ms | first swap 1,939 ms (12.6× rustode, computed); first paint 1,835 ms; first line 1,928 ms | Electron main, renderer, GPU, utility and extension-host processes, the workbench's services and built-in extensions start before the editor shows; rustode starts an editor, tabs, explorer and a status bar (M3 scope). The swap may be one frame early (second swap 1,947 ms) and includes the trace's cost, about 37 ms |
| Startup to first frame, cold cache | 179 ms | first swap 2,184 ms (12.2×); first paint 2,080 ms; first line 2,181 ms | only the editor's own files are dropped from the page cache; rustode's p95 is 334 ms (a few slow launches), the App's 2,213 ms |
| Idle memory (`Pss` / `Rss`) | 103 / 132 MB, 1 process | 853 / 1,605 MB, 8 processes | the App runs features outside Phase 1's scope ([ADR 0018](docs/adr/0018-phase-1-milestones.md)): extensions, terminal, source control |
| Open 1 MB | 151 ms to first frame | first swap 1,931 ms (12.8×) | memory with the file open (`Rss`): 134 MB against 1,362 MB |
| Open 100 MB | 322 ms | first swap 2,390 ms (7.4×) | memory at the first frame 246 MB against 1,512 MB `Rss` |
| Open 1 GB | 2.34 s | first swap 6.36 s (2.7×) | memory at the first frame 1,302 MB against 2,314 MB `Rss`. Both opened it without error; rustode holds the whole file in a rope, so its memory grows with the file. n = 3, rustode's p95 2.62 s |
| 10k random inserts, then undo of all (1 MiB) | inserts 13.8 ms; undo 10.8 ms; to next frame 23.5 ms and 17.9 ms | inserts 9,338 ms; undo 9,774 ms; to next frame 9,490 ms and 10,075 ms | **not like for like (G25)**: same seeded edits and same text after each step (hash), but the App's `TextModel.pushEditOperations` takes a line and column and wakes the open editor's listeners on every insert; rustode's `Workspace::replace` takes a char offset. Peak RSS after: 144 MB against 2,246 MB |
| Regex search, monorepo of 5,000 source files | first result 96.7 ms; complete 96.7 ms | first result 420 ms; complete 532 ms | the same 10,436 matches in 3,002 files on both sides (oracle and `grep` agree); two engines (rustode's walker and `regex`, the App's ripgrep) and two views; warm cache, one query (G26) |
| Typing latency | not measured | not measured | waiting on [ADR 0028](docs/adr/0028-virtual-display-input.md) |

Gaps this suite raised or moved: the App's first presented frame now comes from a trace, with its limits (G22); `rustode-gui` now exits with 1 and a hint when no graphics adapter works (G24, closed); the edit and search scenarios do not separate model from view and are warm (G25, G26); the QA harness's own startup numbers may not be in software (G23, still open).

## Next steps

Study:

1. **Bundle → source map of the main processes** (G1). For each bundle in `out/`, list its entry points in `source/tree/src` and the modules it includes. 6–10 h.
2. **2 h soak** (G4): the only measurement of leaks over time. 2 h of machine time + 1 h of analysis. **Owner decision:** authorize it and choose the time.
3. **Check the `product.json` list of proposed APIs** (G6) against the 1.147 proposals and VSCodium's newest list. 1–2 h. **Owner decision:** download the newest list from the network.
4. **Stress with more free RAM** (G3): open more than 75 files and test files of 10 to 60 MB. 1 h. **Owner decision:** close other apps or lower the soft limit.
5. **License of the analysis documents.** **Owner decision.**
6. **Index the source tree with CodeGraph.** This makes symbol search much faster, since the tree has about 10 thousand `.ts` files. **Owner decision** (indexing is always the owner's choice).
7. **Review the L05 criterion** (G10): baseline after a warm-up cycle. 30 min.

Editor (Phase 2 starts from [finding 0005](09-findings/0005-phase-1-report.md), "Phase 2 starting point", which lists 16 items with their sources):

1. **Owner decisions** (all listed in finding 0005): ADR 0028 (injected input, so typing latency can be measured; [ADR 0025](docs/adr/0025-typing-latency-injected-input.md)); ADR 0027 (keep ropey: finding 0004 has rustode ahead of the App on every buffer scenario); ADR 0029 (extension management); saving over a newer file (ticket 12); `debug.editBreakpoint`; whether commands may write `settings.json`; the split-editor cursor rule; G23; G17 and G18; the editor's `LICENSE` holder; G13; whether CI must be a required check on `main`; the benchmark runner's first-run decisions (one-App guard, wait before shutdown, launch flags, duplicate-version warnings).
2. **Typing latency**, once ADR 0028 is decided (G15, G16).
3. **Never lose work and never crash:** hot exit and backup of unsaved documents, fsync before the atomic rename, `catch_unwind` in `Workspace::execute` and in each frontend frame with a panic hook, mutation tests of the parsers, the missing lints and a layer check (`AGENTS.md` §11.7; none exists today).
4. **Visible editing gaps:** clipboard, line commands, explorer file operations, auto-indent, snippets, code actions, folding, bracket matching, word wrap; the 7 inert settings.
5. **The CLI `open` command** (the one M1 item that did not ship).
6. **Extension host**, the step that makes extension management useful (ADR 0018, ADR 0029).
7. **Benchmark limits:** G22, G25, G26 and a real-GPU run (finding 0004).

## Progress log

- **2026-11-23 (editor, design rules, tickets 04 to 10):** seven more tickets of the plan, each on its own branch. Tickets 04 to 06 reached `main` through #41, 07 through #42, and 09 and 10 through #44. Ticket 08 was merged by #43 into its base branch, `fix/deny-panic-paths`, after #42 had already merged that branch into `main`, so it reached `main` only later, with the typed-error follow-ups below:
  - **04, layers:** `xtask/tests/layers.rs` checks the §11.7.5 table against `cargo metadata`. **05, file size:** `xtask/tests/file_size.rs`, with a temporary allow-list of the 15 files already over 800 lines (ticket 34 removes them). **06, TUI flags:** `rustode-tui` answers `--version` and `--help`; any other argument that starts with `-` is a usage error, exit 2.
  - **07, panic family:** the six-lint `cfg_attr` at every crate root and `xtask/tests/lint_attributes.rs`; the four `unreachable!` became `Result`s. **08, typed extension errors:** `Failure`, `ManifestError` and `SettingValueError` replace `String`; an unreadable `.obsolete` is reported and never overwritten, as `state.json` is. **09, file names in errors:** no `DocumentId` or `GroupId` in a message, swallowed frontend errors shown, a semgrep rule against `{:?}` of errors in frontends, line and column in `when` errors. **10, disk conflicts:** "File: Overwrite" and "File: Discard Your Changes and Revert to File Contents" are registered commands that both frontends offer when a save hits a conflict.
  - **Typed errors, follow-ups to ticket 08:** a listing that finds an unusable `.obsolete` raises `ExtensionEvent::ObsoleteFailed`, shown by the TUI status line and the GUI notice (the file is still never rewritten); `launch.json` problems (`LaunchProblem`), bulk edits (`EditsError`), language-server edits (`AnswerProblem`, `EditFailure`), the TypeScript server lookup (`NoTsserver`) and the replace preview (`LoadError`) replace `String`, and name the file; `Diagnostic` prints its severity as a word.
  - **Deviations:** the hook-fixture fix of `xtask/tests/hooks.rs` was written twice on separate branches; `main` keeps one, where both helpers (`git()` and `xtask()`) remove `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_PREFIX`, `GIT_COMMON_DIR` and `GIT_OBJECT_DIRECTORY`. Ticket 07 left `indexing_slicing` and `string_slice` out on purpose (tickets 11 to 13). Ticket 10 does not register "Compare" (no diff editor).
- **2026-11-23 (editor, design rules):** the plan to adopt filmcraft's design rules and to separate responsibilities between the workspace and the frontends started (39 tickets, from the owner's grilling session of 2026-11-19; the plan and its spec live in the local, unversioned `.scratch/design-rules-adoption/`). Branch `docs/design-rules`, rebased onto `a501bb8`, delivers tickets 01 to 03, docs and checks only, no production code:
  - **03, dependency kinds:** `ATTRIBUTION.md` "Editor dependencies" has a Kind column, the missing `regex` row, and `tracing` lists `rustode-gui`; `xtask/tests/attribution.rs` checks it against `cargo metadata`. The hook fixtures in `xtask/tests/hooks.rs` no longer stage into a linked worktree's index.
  - **02, coverage and setup docs:** `cargo xtask coverage` run on 2026-11-23 (branch tip before rebase onto `a501bb8`), exit 0: core 94.3% of 16486 lines (gate 85%), frontends 91.0% of 14345 lines (gate 60%); README "Get started" and CONTRIBUTING "Setup" rewritten, with the audit corrections.
  - **01, rules:** ADR 0032 (filmcraft's rules adopted: F0.2, F0.4, F0.6 to F0.9, A3.9, the error-message rule) and ADR 0033 (an Activity Bar and a configurable side bar, replacing ADR 0017's "no activity bar"); `AGENTS.md` §11.7 rewritten with every citation re-measured at `8c3c9ab`.
  - **Deviations from the spec:** ADR 0031 was taken (editor groups), so the numbers are 0032 and 0033; the GUI key header's Ctrl+P claim was checked and is correct, so it was left unchanged; the docs index entry for `docs/build.md` was accurate (it describes the App's build), and now says so explicitly.
  - Not done here, by design: every "Not applied yet" in §11.7 names the ticket that applies it.
- **2026-11-23 (editor, M9):** the whole versioned list ran against the App at one commit, with the same work on both sides and a trace for the App's first presented frame (#38, [finding 0004](09-findings/0004-full-app-suite.md)); `rustode-gui` now exits 1 with a hint when no graphics adapter works (G24). The Phase 1 report: milestones M0 to M9 with their pull requests, the Gap Analysis in four parts (36 feature rows, 53 of 53 default keys matching VS Code's, extension management only, TUI and GUI against their criteria), the quality numbers (883 tests, coverage 94.3% core and 91.0% frontends, measured the same day), the owner decisions carried into Phase 2 and the Phase 2 backlog ([finding 0005](09-findings/0005-phase-1-report.md)). This page's status lines, key numbers, milestones, Gap Analysis summary and next steps brought up to `8c3c9ab`; G19 closed.
- **2026-11-19 to 2026-11-23 (editor, M3 to M9):** the GUI (#3, #8, #14, #16, #27), syntax (#12, #23), the LSP client (#10, #24, #28, #29, #33), global search and replace (#15, #30, #32), the DAP client (#26, #31, #34), extension management (#9, #13), editor groups (#35), the micro-benchmark gate (#11), and the first and the full comparison with the App (#36, #38).
- **2026-11-18 (language):** every folder and file name renamed to English, with every link updated: `montar.sh` → `assemble.sh`, `sincronizar-skills.sh` → `sync-skills.sh` (`SEM_FORMATADO` → `SKIP_FORMAT`), `09-analises/` → `09-findings/`, `docs/modelos/` → `docs/templates/`, `docs/componentes/` → `docs/components/`, `docs/aprendizado/` → `docs/learnings/`, `docs/lacunas.md` → `docs/gaps.md`, and the slugs of ADRs 0001–0006 and findings 0001–0002; the scripts' comments and messages are in English. By the owner's explicit decision the evidence folders and generated files were renamed too (`02-package-analysis/`, `03-documents/`, `04-screenshots/`, `07-vscodium-source/`, `08-build-tests/`, `manifest-sha256.txt`, `source/tree/`, `out-formatted/`, `SUMMARY.md`, `network-<run>.md`, …): moved in place, checked against the manifest and by file counts, then rebuilt identically by `assemble.sh`. Only names copied verbatim from the live sources stay in Portuguese.
- **2026-11-17 (editor, M1 and M2):** the view API that frontends draw from, with Enter keeping indentation, Tab following `editor.tabSize` and `editor.insertSpaces`, and only CR, LF and CRLF breaking lines (`a468316`, `a2c25d6`, `865c8fa`); the TUI tracer, with the status bar, the width bands, key-to-frame latency recorded and snapshot tests (`a8c6252`); tabs, the explorer and closing a document, which unwatches its file (`90ebf27`, `3081db1`); quick open with a fuzzy scorer, the command registry, the command palette and go to line (`22f0bbb`, `b813f94`); Dark Modern colors with 24-bit, 256, 16 or no colors (`818073e`); `rustode bench`, the benchmark runner tracer, with its first real run of `open-file-1mb` (`4203f14`, `176081c`, `ea68264`, `8a5ae5b`). This page, the README, ADR 0016, `AGENTS.md` §11.1, `ATTRIBUTION.md` and gaps G17–G18 brought up to `7c2467c`.
- **2026-11-17 (editor, documents):** the Editor section of `AGENTS.md` (§11); ADRs 0007–0028 for the Phase 1 decisions, with 0027 (ropey or a piece tree) proposed and 0028 (the native-input amendment) left as the owner's decision; the contributing guide's review checklist, mirrored by the pull request template; the editor's dependencies in `ATTRIBUTION.md`; this page's milestones and Gap Analysis; gaps G13–G19.
- **2026-11-17 (editor, M1):** editing with undo and redo grouped like VS Code (`042e6fa`); multi-cursor and box selection (`eacae33`); file watching and reload or conflict on external changes (`1fbb01d`, `3e86468`); settings and keybindings in VS Code's format (`46e0234`, `5e2b5ab`); stress budgets, with 1 GB measured at 2.29 s and a 2,208 MB peak (`40cebd1`, `c10e816`); UTF-8 decoded without copying (`9d9313d`; 100 MB peak from 318 MB to 218 MB); the workspace became the only rustode crate the frontends may use (`d6debfb`).
- **2026-11-17 (spike):** keystrokes reach the App and an eframe stand-in on the virtual display through libei and `org_kde_kwin_fake_input` ([0002](09-findings/0002-virtual-wayland-input.md), `a33d58c`). Its exploratory sessions wrote 3 compile-cache files into `01-app/`, which led to the rule against running the App in place (AGENTS.md §11.5; G13).
- **2026-11-17 (documents):** `AGENTS.md` §10 maps every artifact in the evidence folders (`16db07c`).
- **2026-11-16 (editor, M0 and M1):** the rustode folder became a git repository after the owner approved the privacy review (`78aa4d5`); the Cargo workspace with a working `--version` (`569b179`); commit and push hooks backed by xtask (`9022c11`, `a780b8f`); CI and Dependabot (`cd66bc1`); the license policy (`7ee96c3`); shared tracing (`3c8fe23`); open and save keeping encoding and line endings (`c7bfd73`).
- **2026-11-16 (language):** every project document translated to English (`AGENTS.md` now requires it). Folder and file names kept their Portuguese spelling at the time; they were renamed to English on 2026-11-18 (see that entry; the script is now `assemble.sh`).
- **2026-11-16 (tools):** REA 6.8.0 (`rea-agents`) registered as the project's MCP server for Claude Code (`.mcp.json`) and OpenCode (`opencode.json`), with the `reverse-engineer-anything` skill and the rules in `AGENTS.md` §6.
  - Verified: `rea mcp doctor` healthy (229 tools); the `codium` fuses were read; static analysis of `out/main.js` took 21 s, with no writes to 01 to 08.
  - Native decompilation is unavailable, because REA 6.8.0 only accepts Ghidra 12.1.x.
- **2026-11-16 (documents):** rustode gets the documentation set, following the filmcraft standard: `AGENTS.md` (single source; `CLAUDE.md` only imports it), `ROADMAP.md`, `docs/`, ADRs, templates and skills. First analysis recorded (network traffic).
- **2026-11-16 (rustode):** project assembled with real copies (no links) of the app, documents, screenshots, QA, port and analysis material; test results copied to `08-build-tests/`.
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
| 2026-11-23 | minor | Progress-log entry for tickets 04 to 10 of the design-rules adoption plan and the typed-error follow-ups; the other sections were not re-reviewed |
| 2026-11-23 | minor | Progress-log entry for the design-rules adoption plan (tickets 01 to 03); the other sections were not re-reviewed |
| 2026-11-23 | major | Editor: Phase 1 complete. Status lines, key numbers, milestones M0–M9, Gap Analysis summary, Next steps for Phase 2, and the Phase 1 report (finding 0005) |
| 2026-11-18 | minor | Folder and file names renamed to English, evidence folders included |
| 2026-11-17 | minor | Editor state at `7c2467c`: the TUI (M2, in part), `rustode bench`, re-measured numbers, TUI rows in the Gap Analysis, the owner decisions the benchmark runner raised |
| 2026-11-17 | major | Editor: Phase 1 milestones (M0–M9), Gap Analysis against VS Code, editor numbers and next steps; analysis 0002 |
| 2026-11-16 | minor | Translated to English |
| 2026-11-16 | minor | REA registered as a project tool (Claude Code and OpenCode) |
| 2026-11-16 | major | Created from the QA, the port and the build tests |
