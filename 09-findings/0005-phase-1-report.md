# 0005: Where does rustode stand against VS Code at the end of Phase 1? (Gap Analysis, quality numbers and the Phase 2 starting point)

> **Date:** 2026-11-23 · **Confidence:** measured for the counts, the coverage, the shortcut comparison and every cited number of finding 0004; read in code and in the source tree for the feature, extension and UI rows (each row cites its evidence); estimated or hypothesis where a row says so · **Gap:** summarizes G13–G26 and the gaps of the merged pull requests; closes G19 · **Components:** the editor at `8c3c9ab` (`crates/`, `xtask/`, `bench/`), the App (local VSCodium 1.147.0, VS Code `22d8dccf0f29`) and its source tree

## Question

Phase 1 ([ADR 0018](../docs/adr/0018-phase-1-milestones.md)) is ten milestones, M0 to M9, ticket 43 being the last. The owner asks for the Phase 1 report: what is implemented, what is in progress and what is in the backlog against VS Code, in four parts (features, shortcuts, extension compatibility, UI), with what is measured, what is still open for the owner, and where Phase 2 starts.

## Short answer

- **Milestones.** All of M0 to M9 are merged (38 pull requests, 88 commits on `main` at `8c3c9ab`). Two items of the scope lists did not ship and are in the backlog: the CLI `open` command (M1) and, by ADR 0018's own exclusions, everything about running extensions. M2 to M8 each ship with the gaps listed below.
- **Features.** 36 rows: 6 implemented, 22 partial, 4 missing, 4 out of scope for Phase 1 (counted from the table below). What is missing most visibly: copy, cut and paste in the GUI, file operations in the explorer (new, rename, delete), snippets, code actions, folding, word wrap and any Git or terminal support.
- **Shortcuts.** rustode registers 71 commands; 53 have a default key, and all 53 are the key VS Code's source gives for Linux (53 matched, 0 different); 18 have no key, as far as the source shows for the ones checked. VS Code's secondary keys are not registered for 5 of the 53, and about 25 notable VS Code defaults have no command (list below). Several keys need a terminal keyboard protocol that the TUI does not request.
- **Extension compatibility.** Management only: search, install, uninstall, enable and disable against Open VSX, in VS Code's storage format; nothing is loaded and there is no extension host (out of scope for Phase 1, [ADR 0029](../docs/adr/0029-extension-management-open-vsx.md)). Extension-provided features (themes, grammars, snippets, language servers) therefore do not work.
- **UI.** The TUI meets its five acceptance criteria as far as a test backend can show; the GUI meets five of its six in tests (breakpoints, HiDPI, keyboard and AccessKit tree, idle repaint, snapshots), two of them without a real display (fractional scaling, Orca end to end), and the sixth, input-to-photon p99, is not measured.
- **Performance** (finding 0004, not re-measured here): on the virtual display in software rendering, warm startup to the first presented frame is 154 ms against 1,939 ms for the App (12.6 times, computed); idle `Pss` is 103 MB against 853 MB; the 10k-edit scenario, the regex search and the large-file opens are in the table below, with their like-for-like limits (G22, G25, G26). **Typing latency is not measured: waiting on ADR 0028.**
- **Quality.** 883 tests pass and 17 are skipped (`cargo nextest run --workspace --locked`, 2026-11-23, `8c3c9ab`); line coverage 94.3% on the core crates and 91.0% on the frontends (gates: 85% and 60%), measured the same day.
- **Owner decisions carried into Phase 2:** 14, listed under "Open owner decisions".

## Evidence

### Milestones M0 to M9

State at `8c3c9ab`. "PRs" are the merged pull requests (`gh pr list --state merged`); "commits" are the direct commits of the early days (`git log --oneline origin/main`).

| Milestone | State | PRs and commits | What is not done |
|---|---|---|---|
| M0, pipeline and governance | done | commits `78aa4d5`, `569b179`, `9022c11`, `a780b8f`, `cd66bc1`, `7ee96c3`, `2815ed9`, `19c4531`, `8b3b2d1`, `57e45cd`, `a33d58c` (the input spike, [0002](0002-virtual-wayland-input.md)); #1, #2 (CI actions); #4, #7, #17, #18 (Dependabot and Serena, added then removed); #19 (design rules), #20 to #22 (README, wording, English names) | Branch protection exists on `main` (measured, G19 closed); the checks are not listed as required contexts there |
| M1, core and CLI | done except the CLI `open` command | commits `3c8fe23`, `c7bfd73`, `042e6fa`, `eacae33`, `9d9313d`, `40cebd1`, `1fbb01d`, `3e86468`, `46e0234`, `5e2b5ab`, `d6debfb`, `39ca85c`, `cf51f67`, `44ca4a7`; #11 (micro-benchmarks and the regression gate, [ADR 0026](../docs/adr/0026-micro-benchmark-gate.md)) | CLI `open` (`crates/cli/src/main.rs:8-25` knows `--version`, `--help` and `bench` only) |
| M2, workspace and TUI | done | `a8c6252`, `90ebf27`, `3081db1`, `22f0bbb`, `b813f94`, `818073e`, `4e2a0a2`, `53dbb9d`, `706398e`, `7a2b1ca`, `8a6fe63`; the TUI views of #9, #10, #15, #24, #28, #30, #31, #35 | keybinding chord sequences in the user's `keybindings.json`; the gaps of the TUI rows below |
| M3, GUI | done | #3 (tracer), #8 (tabs, explorer, status bar), #14 (breakpoints, HiDPI, focus), #16 (palette, quick input, find), #27 (minimap), #36 (first comparison) | clipboard, hover/status-bar actions and the other gaps below |
| M4, syntax | done | #12 (Rust, incremental), #23 (14 grammars, language detection and picker) | TextMate-grammar parity, bracket pair colors; a typed char on a 100,000-line file misses the 16 ms budget (PR #12) |
| M5, LSP client | done | #10 (servers, diagnostics), #24 (hover, completion), #28 (definition, references, rename, format), #29 (servers started by default), #33 (TypeScript 6) | snippets, code actions, signature help, semantic tokens (rows below) |
| M6, layout and search | done | #15 (search with streaming), #30 (replace with preview and undo), #32 (save only clean files), #27 (minimap), #35 (editor groups, [ADR 0031](../docs/adr/0031-editor-groups.md)) | G20, G21 |
| M7, DAP client | done | #26 (tracer), #31 (stepping, call stack, variables, watch, console), #34 (conditional breakpoints, hit counts, logpoints), [ADR 0030](../docs/adr/0030-dap-client.md) | no attach, no `runInTerminal`, breakpoints do not follow edits (row 20) |
| M8, extension management | done | #9 (Open VSX search, install, uninstall), #13 (enable and disable, VS Code's storage format) | signature verification, updates, details page (below) |
| M9, Phase 1 report | done with this report | #36, #38 (the whole versioned list, [0004](0004-full-app-suite.md)), this pull request | typing latency (ADR 0028) |

### Gap Analysis, part 1: features

Legend: **implemented** (the behavior exists and the listed differences are small), **partial** (a part of what VS Code does is missing), **missing** (nothing yet), **out of scope** ([ADR 0018](../docs/adr/0018-phase-1-milestones.md)). Source-tree paths are relative to `02-package-analysis/source/tree/`. "Gn" is [`docs/gaps.md`](../docs/gaps.md).

| # | Feature | State | rustode (evidence) | Difference from VS Code or what is missing |
|---|---|---|---|---|
| 1 | Open and save with encoding and line endings | partial | BOM, then UTF-8, then Windows-1252; round trips byte-identical for UTF-8 with and without BOM, UTF-16 with BOM, Latin-1, LF, CRLF, mixed (`crates/workspace/tests/open_save.rs:15-29`) | Deliberate fallback to Windows-1252 where VS Code decodes with replacement characters (`src/vs/workbench/services/textfile/common/encoding.ts:88-91`). Missing: UTF-16 without BOM, binary-file detection, `files.autoGuessEncoding` (`encoding.ts:27`, `:477-533`) |
| 2 | Editing, undo and redo | partial | typing groups into undo steps as VS Code does; undo to the saved version is clean; property test (`crates/workspace/tests/editing.rs:135-170`) | Missing: auto-indent from language rules, input-method composition, `undoStopBefore/After` (`src/vs/editor/common/cursor/cursorTypeEditOperations.ts:29`, `:413`, `:573`) |
| 3 | Multi-cursor and box selection | implemented | several selections, box selection, add cursor above/below, `Ctrl+D` (`crates/workspace/tests/multi_cursor.rs`) | Columns count chars not visible columns; `Ctrl+D` is a case-sensitive substring search, not whole word (`src/vs/editor/contrib/multicursor/browser/multicursor.ts:284-321`); no sticky column |
| 4 | Word moves, deletes and line breaks | implemented | word moves and deletes as VS Code's, only CR, LF and CRLF break lines (`crates/workspace/tests/view.rs:49`; `39ca85c`) | no CJK word segmentation |
| 5 | Files changed on disk | partial | clean documents reload as one undo step, dirty ones raise a conflict, deleted/renamed ones stay open and orphaned (`crates/workspace/tests/external_changes.rs`) | A save before the next poll overwrites a newer file; VS Code compares the etag (`src/vs/workbench/services/textfile/common/textFileEditorModel.ts:1036`, `:1088-1090`): **owner decision** (ticket 12). No compare view, no rebase of unsaved edits (`:352-425`); symlinks not resolved; tested on Linux only |
| 6 | Settings and keybindings | partial | VS Code's formats, `when` clauses, `[language]` overrides, live reload (`crates/config/src/settings.rs:100-230`, 21 settings) | 7 of the 21 do nothing yet (`editor.detectIndentation`, `files.encoding`, `files.eol`, `files.trimTrailingWhitespace`, `files.insertFinalNewline`, `files.autoSave`, `files.autoSaveDelay`: `grep` finds them only in `settings.rs`); no settings UI; chord sequences in user keybindings are not applied |
| 7 | Large files | implemented | open 1 MB, 100 MB, 1 GB (finding 0004); 10k edits plus a full undo in 9 ms (`crates/workspace/tests/stress.rs:136-137`) | the whole file is held in a rope (1 GB: 1,302 MB `Rss` at the first frame); no read-only large-file mode |
| 8 | Tabs and explorer | partial | tabs with MRU close, explorer sorted as VS Code's (`crates/workspace/tests/tabs.rs:109`; `crates/workspace/src/explorer.rs:1-14`) | No file operations (new file or folder, rename, delete, copy, drag), no untitled documents, no pinned or preview tabs, no editor picker on `Ctrl+Tab`; the user's `files.exclude` is not read by the explorer (`explorer.rs:13`); locale collation approximated |
| 9 | Quick open, command palette, go to line | partial | fuzzy scorer, `.gitignore`, open editors first (`crates/workspace/tests/quick_open.rs:62`); VS Code's ids and labels (`crates/workspace/src/commands.rs:145`) | palette matches substrings and sorts alphabetically; no recently used first, no word-start match (`src/vs/platform/quickinput/browser/commandsQuickAccess.ts:56`); no `@` symbols or `#` quick access |
| 10 | Find and replace in a file | partial | plain/regex, case, whole word, replace one/all as one undo step, in both frontends (`crates/workspace/tests/find.rs`; `706398e`, `8a6fe63`, #16) | Rust `regex` dialect (no look-around or back-references) where VS Code uses JavaScript's (`crates/workspace/src/find.rs:57-62`); no Find in Selection, Preserve Case or history; the find commands are not in the palette (#16) |
| 11 | Search across files | partial | streaming results, include/exclude globs, `files.exclude`/`search.exclude`, ignore files, 20,000-result limit (`crates/workspace/tests/search.rs`; #15) | no `!` exceptions, no search as you type, no smart case, no size cap, toggle for excludes not in the frontends; whole word uses find's separators (#15) |
| 12 | Replace across files | partial | preview, replace one/file/all, undo of all files, saves only files that were clean (`crates/workspace/tests/search_replace.rs`; #30, #32; [G20](../docs/gaps.md)) | no diff-editor preview; edited files stay open as tabs; files over 64 MiB refused (G20) |
| 13 | Split editors | partial | tree of splits, one document and per-view cursors, `Ctrl+\`, group focus and move commands (`crates/workspace/tests/groups.rs`; [ADR 0031](../docs/adr/0031-editor-groups.md); #35) | no drag of dividers or tabs, no sizes, layout not saved, directional split/move commands missing, inactive GUI pane has no minimap or hover ([G21](../docs/gaps.md)); a cursor at the exact insertion point of another view's typing moves after it, **unverified against VS Code** |
| 14 | Minimap | partial | GUI only: blocks, slider, click and drag (`crates/gui/src/minimap.rs`; #27; the TUI has none by [ADR 0016](../docs/adr/0016-tui-ratatui.md)) | blocks not glyphs; only `editor.minimap.enabled` is a setting; no selection/find/diagnostic marks; not in the AccessKit tree; the toggle is a session override |
| 15 | Syntax highlighting | partial | Tree-sitter, incremental, 14 grammars (Rust, JSON, TOML, Markdown, JavaScript, TypeScript and TSX, Python, C, C++, Go, Bash, YAML, HTML, CSS: `crates/syntax/Cargo.toml:13-27`), Dark Modern token colors, language detection and picker (#12, #23) | VS Code uses TextMate grammars, so token boundaries differ; no bracket pair colors; no semantic tokens; embedded languages not highlighted; a typed char on a 100,000-line file misses 16 ms (#12) |
| 16 | LSP: diagnostics | partial | squiggles, status counts, problems panel; servers supervised, restarted up to 4 times in 3 minutes (`crates/workspace/src/lsp/server.rs:4-22`; #10) | related information and tags ignored; char columns in the list (UTF-16 in VS Code); no `$/progress` display (#29) |
| 17 | LSP: hover, completion | partial | hover and the suggest list, `Ctrl+Space`, typing triggers (#24) | snippet tab stops not supported (inserted as plain text), hover markdown shown as plain text, no resolve or additional edits |
| 18 | LSP: definition, references, rename, format | partial | F12, Shift+F12, F2, `Ctrl+Shift+I` over `WorkspaceEdit` (`crates/workspace/tests/lsp_navigation.rs`; #28) | file operations in an edit refused; no `prepareRename`; no peek; no format selection, on save or on type |
| 19 | Language servers by default | partial | `rust-analyzer` and `typescript-language-server` found on `PATH`, TypeScript 6 handled (`crates/workspace/src/lsp/settings.rs:18`; #29, #33) | one server per language id (TypeScript, TSX, JavaScript, JSX each start theirs); no `rust-analyzer` memory measurement; no code actions, signature help, document symbols, workspace symbols |
| 20 | Debugging (DAP) | partial | `launch.json`, gutter breakpoints, conditional/hit count/logpoints, start, stop, continue, stepping, call stack, variables, watch, console (`crates/workspace/tests/dap.rs`; [ADR 0030](../docs/adr/0030-dap-client.md); #26, #31, #34) | no attach, compound configurations or `runInTerminal`; breakpoints do not follow edits and are not saved; only the stopped thread; no value editing; a real adapter (lldb-dap, debugpy) was tried only in `dap_real.rs` tests; **no mutation tests of the DAP messages** (§11.7.1 rule 7) |
| 21 | Extension management | partial | Open VSX search, install, uninstall, enable/disable, installed list, VS Code's layout (`crates/workspace/src/extensions.rs`; [ADR 0029](../docs/adr/0029-extension-management-open-vsx.md); #9, #13) | see part 3 |
| 22 | Tracing and latency recording | implemented | one setup for every binary, p50/p95/p99 recorder (`crates/trace/src/latency.rs:12`), first-frame report (`crates/trace/src/first_frame.rs`) | the GUI's key-to-present figure is recorded but not compared with the App (ADR 0028) |
| 23 | Benchmarks against the App | implemented | `rustode bench`, list version 3, 8 scenarios, verified App copy, trace of the App's first frame (`bench/scenarios.toml`; finding 0004) | typing latency; G22, G25, G26 |
| 24 | Clipboard (copy, cut, paste) | missing | the TUI accepts bracketed paste (`crates/tui/src/app.rs:364-374`) | nothing in the GUI (`grep -rniE "paste|clipboard" crates/gui/src` finds no handler); no copy or cut in either frontend (PR #3 lists it) |
| 25 | Line and block editing commands | missing | none | VS Code: comment toggle, move/copy/delete line, indent/outdent, select line, join (`src/vs/editor/contrib/comment/browser/comment.ts:89`, `.../linesOperations/browser/linesOperations.ts:201`, `:516`, `:619`) |
| 26 | Code actions, quick fix, signature help, symbols | missing | none (the `docs/gaps.md` and PR bodies do not list them; hypothesis that no request is sent: `grep -rn "codeAction\|signatureHelp\|documentSymbol" crates/workspace/src` prints nothing) | `src/vs/editor/contrib/codeAction/browser/codeActionCommands.ts:80`, `.../parameterHints/browser/parameterHints.ts:84`, `.../quickaccess/gotoSymbolQuickAccess.ts:356` |
| 27 | Folding, bracket matching, word wrap, sticky scroll | missing | none | not in ADR 0018's exclusions and not in its milestone scopes; listed because users expect them (`src/vs/editor/contrib/folding/browser/folding.ts:727`) |
| 28 | Themes and icon themes | partial | Dark Modern only, in the TUI and GUI (`crates/tui/src/theme.rs:11-54`) | the App's default is "Dark 2026" and the owner has not confirmed the choice ([G18](../docs/gaps.md)); no theme picker |
| 29 | Accessibility | partial | AccessKit on in the GUI: editor text input, tab list, explorer tree, status node, quick input as combo box (`crates/gui/src/a11y.rs`; #3, #14) | screen-reader output over AT-SPI/Orca not verified end to end (#3); the minimap and the find toggles differ from VS Code's roles |
| 30 | Integrated terminal | out of scope | none | [ADR 0018](../docs/adr/0018-phase-1-milestones.md) |
| 31 | Source control | out of scope | none | ADR 0018 |
| 32 | Running extensions, TextMate grammars, MCP, remote, Web | out of scope | none | ADR 0018 |
| 33 | Windows and macOS | out of scope | Linux x64 only ([ADR 0012](../docs/adr/0012-linux-x64-only.md)) | ADR 0018 |
| 34 | CLI | partial | `rustode --version`, `--help`, `bench` (`crates/cli/src/main.rs:8-25`) | no `open` command (M1 scope); `rustode-protocol` is a one-line placeholder (`crates/protocol/src/lib.rs`), no transport in Phase 1 |
| 35 | Crash resilience and never losing work | partial | `Result` everywhere, atomic save through a temporary sibling (`crates/fs/src/io.rs:15-36`), supervised workers (§11.7.1 rules 1 to 6) | `AGENTS.md` §11.7 "not applied yet", all re-checked on 2026-11-23: no `catch_unwind` (`grep -rn catch_unwind crates` prints nothing), no panic hook, no fsync (`grep -rn "sync_all\|fsync" crates/fs/src` prints nothing), no hot exit or backup of unsaved documents, no mutation tests of the parsers, no layer-check task |
| 36 | Idle and repaint behavior | implemented | the GUI schedules no frame when idle (`crates/gui/tests/frame/editor.rs:386-400`); the TUI draws cell diffs | idle CPU and GPU were not measured on a real display |

Counts by the state column (counted by a script that reads the table): 6 implemented, 22 partial, 4 missing (rows 24 to 27), 4 out of scope (rows 30 to 33), 36 in all.

### Gap Analysis, part 2: shortcuts

**Method.** `COMMANDS` (`crates/workspace/src/commands.rs:145`) lists 71 commands (counted by a script that reads the table: 71; 53 with a default key, 18 without). For each of the 53, the `keybinding` block of the command in the source tree was read, for Linux (`primary`, with `linux:` where it overrides). This is a hand comparison of each block, not an automatic one. For the 18 without a key, the definitions of `workbench.action.focusNextGroup`, `focusPreviousGroup`, `joinAllGroups` (`src/vs/workbench/browser/parts/editor/editorActions.ts:227`, `:327`, `:339`), `splitEditorRight` and `splitEditorDown` (`:140-184`), `toggleMinimap` (`src/vs/workbench/contrib/codeEditor/browser/toggleMinimap.ts:15`) and the three breakpoint editors (`src/vs/workbench/contrib/debug/browser/debugEditorActions.ts:101-215`) have no key binding; the other 8 (rustode's two own commands, the two watch commands and the four `focus…View` commands) were not checked, and are listed as unknown.

| Result | Count | Which |
|---|---|---|
| **Matched** (rustode's key equals VS Code's Linux `primary`) | **53** | Ctrl+S (`src/vs/workbench/contrib/files/browser/fileCommands.ts:504`), Ctrl+Q (`src/vs/workbench/electron-browser/desktop.contribution.ts:88-90`), Ctrl+W twice with VS Code's two `when` clauses (`src/vs/workbench/browser/parts/editor/editorCommands.ts:841`, `:869`), Ctrl+K W (`:856`), Ctrl+\ (`editorActions.ts:91`), Ctrl+1 to Ctrl+3 (`editorActions.ts:304`; `editorCommands.ts:668-669`), the four Ctrl+K Ctrl+arrow focus chords (`editorActions.ts:356-404`), Ctrl+Alt+Left/Right (`:2132`, `:2152`), Ctrl+PageDown/PageUp (`:1277`, `:1325`), Ctrl+Shift+E (`explorerViewlet.ts:271`), Ctrl+Shift+X (`extensions.contribution.ts:121`), Ctrl+Shift+F and Ctrl+Shift+H (`searchActionsFind.ts:241`, `searchActionsNav.ts:402`), Ctrl+Shift+1 for the two replace commands and Delete for dismiss (`searchActionsRemoveReplace.ts:66`, `:160`, `:196`), F6 (`navigationActions.ts:367`), Ctrl+P (`quickAccessActions.ts:26`), Ctrl+Shift+P (`commandsQuickAccess.ts:284`), Ctrl+G (`gotoLineQuickAccess.ts:83`), Ctrl+Z, Ctrl+Y, Ctrl+A (`src/vs/editor/browser/editorExtensions.ts:651`, `:678`, `:708`), Ctrl+D (`multicursor.ts:734`), Shift+Alt+Up/Down (`:57`, `:109`), Ctrl+Home/End (`src/vs/editor/browser/coreCommands.ts:1247`, `:1291`), Ctrl+Shift+M (`markers.contribution.ts:153`), Ctrl+Space (`suggestController.ts:807`), Ctrl+K Ctrl+I (`hoverActions.ts:65`), F12, Shift+F12 (`goToCommands.ts:292`, `:681`), F2 (`rename.ts:370`), Ctrl+Shift+I on Linux (`formatActions.ts:224`), Ctrl+K M (`editorStatus.ts:1121`), F5 start and continue, Shift+F5, F9, F10, F11, Shift+F11, F6 pause, Delete for the watch (`debugCommands.ts:687`, `:767`, `:650`, `debugEditorActions.ts:50`, `debugCommands.ts:504`, `:517`, `:538`, `:553`, `:892`) |
| **Different** (a key that is not VS Code's) | **0** | none found |
| **No key in rustode, none in VS Code** (checked) | 10 | `focusNextGroup`, `focusPreviousGroup`, `joinAllGroups`, `splitEditorRight`, `splitEditorDown`, `toggleMinimap`, `conditionalBreakpoint`, `addLogPoint`, `editBreakpoint`, plus `search.action.replaceAll` is a view button, not a key (`searchView.ts:814-856`) |
| **No key in rustode, not checked in VS Code** | 8 | `rustode.search.undoReplace`, `rustode.restartLanguageServer` (rustode's own), `addWatchExpression`, `removeAllWatchExpressions` and the four `focus…View` debug commands |
| **VS Code's secondary key not registered** | 5 | Ctrl+E for quick open (`quickAccessActions.ts:27`), Ctrl+Shift+Z for redo (`editorExtensions.ts:679`), Ctrl+Shift+Up/Down for the cursor commands (`multicursor.ts:58`, `:110`), F1 for the palette (`commandsQuickAccess.ts:285`); the frontends handle F1, Ctrl+Shift+Z and Ctrl+Shift+Up/Down themselves (`crates/gui/src/keys.rs:139-142`), the registry does not list them |

**Notable VS Code defaults with no command in rustode** (Linux, from the source tree; "frontend" means a frontend handles the key but the registry has no command):

| Key | VS Code command | State in rustode |
|---|---|---|
| Ctrl+C, Ctrl+X, Ctrl+V | clipboard (`src/vs/editor/contrib/clipboard/browser/clipboard.ts:48`, `:85`) | missing in the GUI; TUI paste only through the terminal |
| Ctrl+F, Ctrl+H, F3, Shift+F3 | find and replace (`findController.ts:572`, `:783`) | frontend only (`crates/gui/src/keys.rs:43-45`), not in the palette |
| Ctrl+B | toggle sidebar | GUI frontend only |
| Ctrl+Tab | editor picker | GUI switches to the previous tab without a picker |
| Ctrl+N, Ctrl+O, Ctrl+Shift+T | new file (`fileCommands.ts:702`), open, reopen closed editor (`editorActions.ts:1703`) | missing |
| Ctrl+/ | toggle line comment (`comment.ts:89`) | missing |
| Alt+Up/Down, Ctrl+Shift+K, Ctrl+], Ctrl+[ | move, delete line, indent (`linesOperations.ts:201`, `:516`, `:619`) | missing |
| Ctrl+Shift+O, Ctrl+T, Ctrl+Shift+Space, Ctrl+., F8 | symbols, workspace symbols, parameter hints, quick fix, next problem (`gotoSymbolQuickAccess.ts:356`, `parameterHints.ts:84`, `codeActionCommands.ts:80`, `gotoError.ts:249`) | missing |
| Ctrl+Shift+[ , Ctrl+Shift+] | fold, unfold (`folding.ts:727`) | missing (no folding) |
| Ctrl+` | terminal (`terminal.contribution.ts:130`) | out of scope |
| Alt+F12, Ctrl+K F12 | peek definition, definition to the side (`goToCommands.ts:362`, `:331`) | missing (no peek) |
| Shift+F6 | previous part (`navigationActions.ts:383`) | not registered |

**Keys that do not reach a terminal.** The TUI does not request the kitty keyboard protocol (`grep -rn KeyboardEnhancement crates/tui/src` prints nothing), so in a plain terminal these defaults arrive as other keys or not at all: Ctrl+Shift+E (arrives as Ctrl+E; F6 moves focus instead), Shift+F12 and Ctrl+Shift+I (PR #28), Ctrl+Shift+1 (arrives as Ctrl+Enter, PR #30), Ctrl+1 to Ctrl+3, Ctrl+Alt+arrows and Ctrl+\ (arrives as Ctrl+4; PR #35). The palette runs every command in those cases. In the GUI the keys come through winit with their modifiers.

### Gap Analysis, part 3: extension compatibility

Phase 1 manages extensions and never runs them ([ADR 0018](../docs/adr/0018-phase-1-milestones.md); [ADR 0029](../docs/adr/0029-extension-management-open-vsx.md)). **Proposed, not accepted** — ADR 0029 has four owner decisions (storage folders, refusing releases without a SHA-256, the API version the manifests are checked against, the global-storage folder).

| Aspect | State | Evidence |
|---|---|---|
| Search, install, uninstall, installed list against Open VSX (never the Microsoft Marketplace, [§1.2](../AGENTS.md)) | implemented | `crates/workspace/src/extensions.rs:60` (`OPEN_VSX`); local stand-in in tests (`crates/workspace/tests/extensions.rs`); #9 |
| Enable and disable, globally, in VS Code's `extensions.json` and state keys, unknown keys kept | implemented | `crates/workspace/src/extensions/state.rs:16-19`; #13 |
| Manifest model and validation against VS Code API 1.147.0 (`VSCODE_API_VERSION`) | implemented | `crates/workspace/src/extensions/manifest.rs`; ADR 0029 decision 4 |
| Extension host, activation events, contributions (commands, themes, grammars, snippets, language servers, debuggers), the `vscode` API | out of scope | ADR 0018; nothing is loaded (`NOT_LOADED`, re-exported at `crates/workspace/src/lib.rs`) |
| Signature verification | missing | the App uses `@vscodium/vsce-sign`; rustode checks SHA-256 only (#9) |
| Updates, pre-release, filters, paging, details page, workspace-level enablement, dependency and pack cascade, allowlist and malicious states | missing | #9, #13 |
| Compatibility in practice | unknown | not run against the real Open VSX; that its hosts match the App's is a hypothesis (#9, ADR 0029) |
| Package safety | implemented | own caps: 512 MiB download, 100,000 entries and 1 GiB extraction (#9); `crates/workspace/src/extensions.rs` |

What this means for users: an installed extension appears in the list and can be enabled or disabled, and its manifest is validated, but none of its features work. Phase 2 needs the host before this part becomes useful.

### Gap Analysis, part 4: UI against the acceptance criteria

TUI ([ADR 0016](../docs/adr/0016-tui-ratatui.md), five criteria):

| Criterion | State | Evidence |
|---|---|---|
| Width bands: sidebar at 120+, collapsed 80–119, editor only below 60, "too small" below 40×10; resize without stale cells | met; 60–79 columns is undefined in the ADR | `crates/tui/tests/tui.rs:90` (`each_width_band_has_its_layout`), 52 snapshots (`crates/tui/tests/snapshots/`), `crates/tui/src/view.rs:32-68`; [G17](../docs/gaps.md) is an owner decision |
| Key-to-frame p99 under 16 ms | met on ratatui's test backend only | `crates/tui/tests/tui.rs:216-234`; not measured through a terminal emulator |
| VS Code keybindings, mouse, palette, Dark Modern | met in part | part 2; mouse in `4e2a0a2`; user keybindings applied (`53dbb9d`) without chord sequences |
| Color fallback, `NO_COLOR`, no meaning by color alone | met | `crates/tui/tests/tui.rs:523` (`no_color_draws_rust_without_token_colors`), `crates/tui/src/theme.rs:11-54` |
| Snapshot tests at each width band | met | 52 text snapshots; band tests in `tui.rs:90`, `groups.rs:112` |

GUI ([ADR 0017](../docs/adr/0017-gui-egui-wgpu.md), six criteria):

| Criterion | State | Evidence |
|---|---|---|
| VS Code's width breakpoints for the sidebar and status bar | met, with a deliberate difference (the sidebar collapses below 390 points; VS Code never does) | `crates/gui/src/layout.rs`, `crates/gui/tests/frame/layout.rs`; #14 |
| Input-to-photon p99 under 16 ms at 60 Hz | **not measured**: the present is timed in-process (`crates/gui/src/latency.rs`, `crates/gui/tests/latency.rs`), the compositor and display are not; real keys need ADR 0028 | `crates/gui/examples/typing.rs`; ADR 0028 |
| Near-zero idle CPU and GPU | met by construction and by test (no frame scheduled when idle); not measured as CPU or GPU use | `crates/gui/tests/frame/editor.rs:386-400`; finding 0004 idle memory only |
| HiDPI and fractional scaling | met in tests at scales 1, 1.25, 1.5 and 2; fractional scaling on a real display not checked | `crates/gui/tests/frame/scale.rs:90`, `:161`, `:180`; #14 |
| Full keyboard navigation and AccessKit exposing the UI to Orca | met in tests (focus order, F6, Tab, AccessKit tree); Orca end to end not verified | `crates/gui/src/focus.rs`, `crates/gui/src/a11y.rs`, `crates/gui/tests/frame/keyboard.rs`; #3, #14 |
| Snapshot tests with egui_kittest including the AccessKit tree | met | 58 image snapshots (`crates/gui/tests/snapshots/`), 159 GUI tests |

TUI specifics that differ from VS Code (from the Phase 1 roadmap rows, still true): from 60 to 79 columns the TUI shows what it shows from 80 to 119 (G17, `crates/tui/src/view.rs:32-68`); no current-line highlight (`src/vs/editor/common/config/editorOptions.ts:6739`), no "(n selected)" after the position (`src/vs/workbench/browser/parts/editor/editorStatus.ts:338-340`), no indentation indicator (`editorStatus.ts:803-806`); 256 and 16 colors approximate the theme (`crates/tui/src/theme.rs:11-54`); the explorer's names are compared by an approximation of the locale's collation (`crates/workspace/src/explorer.rs:4-9`, `src/vs/base/common/comparers.ts:11-16`).

Other UI gaps: no activity bar, file icons, hover highlight, drag-to-resize sidebar or panels, status-bar actions, narrow find layout (#8, #14, #16); no CJK or emoji font (#3); the GUI's inactive pane has no minimap or hover (G21).

### Performance against the App

From [finding 0004](0004-full-app-suite.md), measured on 2026-11-23, release `rustode-gui`, both editors on `kwin_wayland --virtual` in software rendering, the App from a verified copy, `rustode` at `6d7b04e50f98`; not re-measured for this report. Medians; the finding has n, p95 and every definition.

| Scenario | rustode | App | Caveat |
|---|---|---|---|
| Startup warm, to first frame (n = 20) | 154 ms | first swap 1,939 ms (12.6×, computed) | G22: the App's event is a swap after a trace; may be one frame early; carries the trace's cost (about 37 ms in one control pair) |
| Startup cold (n = 20) | 179 ms | 2,184 ms (12.2×) | only the editor's own files are evicted |
| Idle `Pss` / `Rss` (n = 5) | 103 / 132 MB | 853 / 1,605 MB | the App runs extensions, terminal and source control |
| Open 1 MB · 100 MB · 1 GB | 151 ms · 322 ms · 2.34 s | 1,931 ms · 2,390 ms · 6.36 s | n = 5, 5, 3 |
| 10k inserts, then undo of all | 13.8 ms · 10.8 ms | 9,338 ms · 9,774 ms | **not like for like (G25)** |
| Regex search over 5,000 files | first 96.7 ms, complete 96.7 ms | 420 ms, 532 ms | two engines, warm cache (G26); same 10,436 matches |
| Typing latency | **not measured: waiting on ADR 0028** | not measured | |

What the numbers say about [ADR 0027](../docs/adr/0027-ropey-or-piece-tree.md): its trigger is "rustode behind the App in a buffer scenario, and a profile blaming the rope". Finding 0004 has rustode ahead of the App in every buffer scenario it ran (open 1 MB, 100 MB, 1 GB; 10k edits then undo, with the G25 caveat), so the first condition did not fire; no profile of the 1 GB memory (about 1.3 times the file's size in `Rss` at the first frame) exists. The evidence supports accepting "keep ropey" for Phase 1; the decision is the owner's.

### Quality numbers

| What | Value | How |
|---|---|---|
| Tests | 883 pass, 17 skipped | measured, 2026-11-23, `8c3c9ab`, debug build, rustc 1.99.1: `cargo nextest run --workspace --locked` |
| Line coverage, core crates | 94.3% of 16,486 lines (gate 85%) | measured, 2026-11-23, `8c3c9ab`: `LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo xtask coverage` |
| Line coverage, frontends | 91.0% of 14,345 lines (gate 60%) | the same run |
| Rust source | 77,218 lines in `crates/` and `xtask/` | measured: `find crates xtask -name "*.rs" \| xargs cat \| wc -l` |
| Direct third-party dependencies | 33 normal (40 direct normal dependencies of the 12 crates, 7 of them rustode crates); 546 packages in the graph (all targets and kinds) | measured: `cargo metadata --format-version 1 --locked --offline`, 2026-11-23 |
| Decisions | 31 ADRs: 6 for the study, 25 for the editor; of the editor's, 22 accepted and 3 proposed (0027, 0028, 0029) | counted: [`docs/adr/README.md`](../docs/adr/README.md) |
| CI | checks, coverage, security, bench-gate, conventions on every pull request | PR #38 |

Not run: the micro-benchmark gate against `main` (CI's `bench-gate` job ran green on #38); mutation tests (none exist).

## Open owner decisions

Carried into Phase 2, as of 2026-11-23:

1. **ADR 0028**, injected input on the virtual display for typing latency (proposed; amends [ADR 0006](../docs/adr/0006-tests-on-virtual-display.md) rule 2 and AGENTS.md §3.2). Until accepted, typing latency is not measured and the GUI's 16 ms criterion has no end-to-end number.
2. **ADR 0027**, ropey or a piece tree (proposed). Finding 0004 has rustode ahead of the App on every buffer scenario; accept "keep ropey", or ask for the memory profile first.
3. **ADR 0029**, extension management (proposed): the extensions and global-storage folders, refusing releases without a SHA-256, the API version to validate against.
4. **Split editors:** a cursor sitting at the exact insertion point of another view's typing moves after it; unverified against VS Code (G21, ADR 0031).
5. **`debug.editBreakpoint`:** VS Code's id is the function breakpoint's; rustode registered `editor.debug.action.editBreakpoint` for the editor action. Add `debug.editBreakpoint` beside it, or not (PR #34).
6. **G23:** re-run one QA startup benchmark forced to software rendering to see whether the 2026-11-13 and 2026-11-16 QA numbers used the GPU.
7. **Settings written by commands:** whether a command may write the user's `settings.json` (today the minimap toggle is a session override, PR #27).
8. **Saving over a newer file** (ticket 12): compare the disk stamp at save and raise a conflict, as VS Code does with etags. It changes the open-and-save round-trip test (`crates/workspace/tests/open_save.rs:48-54`).
9. **The benchmark runner's first-run decisions** (still open: no later record found): keep or narrow the one-App guard (it refused a re-run of this ticket's battery while the owner's own VSCodium was open; `crates/cli/src/bench/guard.rs:82-133`), the wait before shutdown, whether to record the first run, the App's launch flags, the duplicate `syn` warning.
10. **G17 and G18:** the TUI layout from 60 to 79 columns, and Dark Modern against the App's "Dark 2026" together with whether theme color values may be reused.
11. **The editor's `LICENSE`:** its copyright line names the holder ([ADR 0009](../docs/adr/0009-mit-and-reimplemented-behavior.md)).
12. **G13:** make the integrity check see added files in §0.6 and the `compare-builds` skill, and set `NODE_DISABLE_COMPILE_CACHE=1` in the launch skill and the QA launchers (`rustode bench` already does both on its copy).
13. **Required status checks on `main`:** branch protection exists (linear history, no force push, strict checks), but its list of required checks is empty (measured with `gh api repos/<repo>/branches/main/protection`, 2026-11-23); decide whether CI must be required.
14. **Study items carried over:** the 2 h soak (G4), the `product.json` proposed-API list (G6), stress with more RAM (G3), the license of the analysis documents, indexing the source tree with CodeGraph (ROADMAP "Next steps", study).

## Phase 2 starting point

A prioritized backlog derived from the gaps. Priority is the order of value to a user, then of risk; the estimates are not made here.

| # | Item | Why | Source |
|---|---|---|---|
| 1 | **Typing latency** against the App | the last unmeasured ADR 0024 scenario; the GUI's 16 ms criterion | ADR 0028 (decision 1), [ADR 0025](../docs/adr/0025-typing-latency-injected-input.md), G15, G16 |
| 2 | **Never lose work**: hot exit and backup of unsaved documents, fsync before the rename, save-over-newer conflict | the design rules' "not applied yet" list; ticket 12 | `AGENTS.md` §11.7.1 rule 6, decision 8 |
| 3 | **Last-resort layers**: `catch_unwind` in `Workspace::execute` and in each frontend frame, a panic hook with a crash log | none exist (re-checked) | §11.7.2 layers 4 and 6 |
| 4 | **Mutation tests** for JSONC, `when` clauses, VSIX, manifests, LSP and DAP messages | none exist; two PRs flagged it | §11.7.1 rule 7; PRs #26, #31 |
| 5 | **Lint and layer checks**: `expect_used`, `panic`, `unreachable`, `unimplemented`, `indexing_slicing`; a table-driven layer check | the rule is stricter than the lints | §11.7.1 rule 2, §11.7.5 |
| 6 | **Clipboard** in the GUI and copy/cut in both frontends; the missing line commands (comment, move, delete, indent) | the most visible editing gap | features rows 24, 25; part 2 |
| 7 | **Explorer file operations** (new, rename, delete, drag) and untitled documents | the most visible workbench gap | features row 8 |
| 8 | **CLI `open`** (M1 scope) and a protocol transport decision | M1 left it | features row 34; [ADR 0014](../docs/adr/0014-in-process-frontends.md) |
| 9 | **Extension host**: activation events, the `vscode` API subset, contributions (themes, grammars, snippets, language servers) | without it extension management has no effect | part 3; ADR 0018, ADR 0029 |
| 10 | **Editing quality**: auto-indent, snippets (LSP completion), code actions, signature help, symbols, folding, bracket matching, word wrap | what VS Code users use every minute | feature rows 2, 17, 26, 27 |
| 11 | **Settings that do something**: `files.eol`, `files.trimTrailingWhitespace`, `files.insertFinalNewline`, `files.autoSave`, `editor.detectIndentation`, `files.encoding`; chord sequences in user keybindings | 7 of 21 settings are inert | row 6 |
| 12 | **Search and find parity**: the JavaScript regex dialect or a documented difference, Find in Selection, Preserve Case, search as you type, recently used commands first in the palette | feature rows 9 to 11 | G26 |
| 13 | **Editor groups**: drag, sizes, layout saved, directional commands | G21 | row 13 |
| 14 | **Benchmarks**: split the App's model from its view (G25), a cold and larger search (G26), the first-frame check (G22), a real-GPU run, the micro-benchmark gate on more crates | the comparison's open limits | finding 0004 |
| 15 | **DAP follow-ups**: attach, `runInTerminal`, breakpoints that follow edits and persist, per-thread stepping | M7 left them | row 20; PRs #26, #31, #34 |
| 16 | **Accessibility on real hardware**: Orca end to end, fractional scaling on a display, a visual check of the minimap | tests cannot show them | part 4; PRs #3, #14, #27 |

## Limitations

- The feature, extension and UI rows are read from the code, the tests, the ADRs, `docs/gaps.md`, `ATTRIBUTION.md` and the merged pull requests' "Gaps" sections; none of them was run against the App except the numbers of finding 0004. A feature VS Code has that no document lists is missing from this analysis; rows 24 to 27 are the ones found while comparing shortcuts and are not a complete list.
- The shortcut comparison is by hand from the `keybinding` blocks and is limited to Linux and to commands rustode registered; VS Code has about 1,000 default keybindings in the source tree that this report does not enumerate. "Matched" says the key equals VS Code's, not that the command behaves the same.
- Counts of features by state depend on this report's definitions of the four states.
- The coverage figures come from one run of `cargo xtask coverage` under rustc 1.99.1; the tool was reported distorted under rustc 1.100 (PR #12).
- No performance number here is new; finding 0004's limits apply.

## Next steps

- The owner decides the list above; item 1 unlocks Phase 2's first benchmark, item 8 and 9 shape its first tickets.
- Phase 2 tickets follow the backlog table; each starts from the row it cites.
