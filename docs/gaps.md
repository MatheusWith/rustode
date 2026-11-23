# Gaps and open questions

> **Last reviewed:** 2026-11-17 · **Last updated:** 2026-11-23 · **Change:** minor (G22 and G24 updated, G25 and G26 added) · **Target:** local VSCodium 1.147.0 and the rustode editor, Phase 1

This page lists what is still unknown or has not been measured, in priority order: first the study (G1–G12), then the editor (G13 on). The hour figures are estimates, not measurements. When a gap is resolved:
- record the finding in `09-findings/`;
- mark the row as resolved;
- point the row to the finding.

| # | Gap | Type | Blocks active analysis | Estimate |
|---|---|---|---|---|
| G1 | No bundle → source-module map | analysis | yes | 6–10 h |
| G2 | Bundles without source maps | build limitation | no (can be worked around) | — |
| G3 | Stress limited by free RAM | test | no | 1 h |
| G4 | 2 h soak not run | test | no | 3 h |
| G5 | About (M17) is a native dialog, outside CDP | test | no | 30 min |
| G6 | The `product.json` list of proposed APIs is from 1.143 | compatibility | no | 1–2 h |
| G7 | Secondary sidebar empty on first open | behavior | no | 1 h |
| G8 | SIGTERM to all processes leaves the app stuck | inherited behavior | no | 2–4 h |
| G9 | Electron dictionary downloaded from Google at startup | privacy | no | 1 h |
| G10 | Unsuitable L05 criterion | test | no | 30 min |
| G11 | A full typecheck of `src` has never run | build validation | no | depends on RAM |
| G12 | 25 unresolved imports in the tests (19 in the original) | build validation | no | 1–2 h |

Editor (rustode, Phase 1):

| # | Gap | Type | Blocks | Estimate |
|---|---|---|---|---|
| G13 | The integrity check does not see files added to `01-app/` | evidence protection | no | 30 min |
| G14 | Does a relocated copy of the App reuse its Node compile cache? | benchmark validity | startup comparisons ([ADR 0024](adr/0024-comparison-with-the-app.md)) | 1 h |
| G15 | Which App-side event marks a presented frame? | benchmark method | typing latency and startup scenarios ([ADR 0024](adr/0024-comparison-with-the-app.md), [0025](adr/0025-typing-latency-injected-input.md)) | 2–4 h |
| G16 | The App lost the first injected keystroke once | benchmark method | typing latency ([ADR 0025](adr/0025-typing-latency-injected-input.md)) | 1–2 h |
| G17 | The TUI layout from 60 to 79 columns is undefined | specification | no; the TUI made a provisional choice (M2) | — |
| G18 | Dark Modern or the App's default theme, and may theme colors be reused? | specification, license | the TUI uses Dark Modern provisionally (M2); GUI colors (M3), token colors (M4) | — |
| G19 | Is branch protection available for the private repository? | repository | no | 15 min |

## G1. No bundle → source-module map

- **Missing:** for each large bundle (`workbench.desktop.main.js`, `sessions.desktop.main.js`, `agentHostMain.js`, `extensionHostProcess.js`), which `src/` modules it contains and in what order. Today only the entry file is known ([`source-vs-package.md`](source-vs-package.md) §1).
- **Impact:** without it, locating a minified snippet takes a case-by-case anchor search.
- **Approach:** one of three options. **6–10 h.**
  - Generate the esbuild metafile from the tree. The `metafile` option is already used in the mobile build in `build/next/index.ts`.
  - Build an unminified bundle of the same entry points, just for mapping.
  - Use REA's `import_reference_source` + `compare_source_to_bundle` (AGENTS.md §6).

## G2. Bundles without source maps

- **Missing:** the maps of the main bundles (ADR 0001).
- **Impact:** stack traces and minified snippets don't point straight to the source.
- **Approach:** one of two options.
  - Anchors + the source tree (`find-minified-symbol` skill).
  - A build with maps on a machine with more RAM, comparing the JS against the manifest to make sure it is the same code.

## G3. Stress limited by free RAM

- **Missing:** two things that did not run.
  - Opening more than 75 files: X01 stopped at the soft limit of 1.2 GB free.
  - Files of 10, 30 and 60 MB: X03 did not run.
- **Impact:** the app's degradation point on this machine is unknown.
- **Approach:** run `explore_stress.cjs` with other apps closed or with a lower `QA_STRESS_SOFT_MB`; the 700 MB watchdog stays. **Owner decision.** **1 h.**

## G4. 2 h soak not run

- **Missing:** the slope of heap, listeners and nodes over time.
- **Impact:** it is the only leak measurement; L05 is not suited to it (G10).
- **Approach:** `explore_soak.cjs` with `QA_SOAK_MIN=120` and `QA_MAX_SEC` above that. **Owner decision** (it ties up the machine). **3 h.**

## G5. About (M17) is a native dialog

- **Missing:** checking the contents of the "About" window (version, commit, Electron).
- **Approach:** one of two options. **30 min.**
  - A native screen capture, which needs the owner's authorization to take over the screen.
  - Reading `product.json` and `version.txt`, which already give the same values.

## G6. The `product.json` list of proposed APIs is from 1.143

- **Missing:** whether the current versions of the 65 extensions listed in `extensionEnabledApiProposals` request proposals that are not on the list. Today only the converse is known: 3 of the 305 pairs point to proposals that no longer exist in 1.147 (`agentsWindowActivation`, `timeline`).
- **Approach:** compare with two lists. **1–2 h.**
  - VSCodium's newest list, which has to be downloaded from the network: **owner decision**.
  - `src/vs/platform/extensions/common/extensionsApiProposals.ts`.

## G7. Secondary sidebar empty on first open

- **Missing:** two confirmations.
  - Whether official VSCodium also shows the bar empty.
  - Whether the "it was empty" rule (`AUXILIARYBAR_EMPTY`) hides it even on the second open.
- **Evidence:** the block that tests `ChatAIDisabledSettingId` in `layout.ts` (source/tree/src/vs/workbench/browser/layout.ts); the screenshot `…-codium1.147-menus/screenshots/M12.png`.
- **Approach:** open the same profile twice and compare. Known workaround: `workbench.secondarySideBar.defaultVisibility: hidden`. **1 h.**

## G8. SIGTERM to all processes leaves the app stuck

- **Missing:** the cause.
  - Sending SIGTERM only to the main process exits in under 1 s.
  - Sending it to all processes at once leaves 3 processes (4 on 1.138) and requires SIGKILL.
- **Approach:** `strace` on the main process during the mass SIGTERM; read the signal handling in `src/vs/code/electron-main/`. **2–4 h.**

## G9. Electron dictionary downloaded from Google

- **Missing:** two answers.
  - Whether it can be turned off without a patch, through the Electron spell-checker configuration.
  - Whether official VSCodium does the same.
- **Evidence:** [`09-findings/0001-network-traffic.md`](../09-findings/0001-network-traffic.md).
- **Approach:** search the tree for `setSpellCheckerDictionaryDownloadURL`/`spellcheck` and test with the logging proxy. **1 h.**

## G10. Unsuitable L05 criterion

- **Missing:** a criterion that tells "views that stay created" apart from a leak.
- **Approach:** take the baseline after a warm-up cycle (open and close search, terminal and editors once). **30 min.**

## G11. A full typecheck of `src` has never run

- **Missing:** the full typecheck does not fit in 7.5 GiB. Two smaller checks were done instead:
  - a typecheck restricted to the touched files, with no port-only error;
  - the import scan.
- **Approach:** run it on a machine with more RAM, or in parts (smaller TypeScript projects). See `06-port/PORT-NOTES.md`, "Validação".

## G12. Unresolved imports in the tests

- **Missing:** 25 relative imports don't resolve in the tests, against 19 in the original VS Code. In production there are 3, all already present in the original. They stay out of the bundle but break the affected unit tests.
- **Approach:** list the 6 extra ones with `06-port/check-imports.py` and link each to the patch that removed its target (probably 52/53, Copilot). **1–2 h.**

## G13. The integrity check does not see files added to `01-app/`

- **Missing:** `sha256sum -c` (§0.6) checks only the files the manifest lists. Files added to `01-app/` go unnoticed. On 2026-11-17, running the App in place wrote 3 new Node compile-cache files under `resources/app/node-compile-cache/extension-host/`, and the check still passed ([`09-findings/0002`](../09-findings/0002-virtual-wayland-input.md), "Limitations").
- **Impact:** §0.6 can pass while 01 no longer matches the analyzed app.
- **Evidence today:** the file list matched the manifest on 2026-11-17 (3,008 files). Measured from `01-app/VSCode-linux-x64` with `diff <(find . -type f | sort) <(sed 's/^[0-9a-f]*  //' ../../02-package-analysis/manifest-sha256.txt | sort)`, which printed nothing.
- **Approach:** two changes, both proposed by finding 0002. **Owner decision**, because they change §0.6 and a skill. **30 min.**
  - Add the file-list comparison above to §0.6 and to the `compare-builds` skill. Editor benchmarks already run it on their copy (AGENTS.md §11.5).
  - Set `NODE_DISABLE_COMPILE_CACHE=1` in the `launch-built-app` skill and the QA launchers.

## G14. Does a relocated copy of the App reuse its Node compile cache?

- **Missing:** editor benchmarks run a verified copy of the App from another folder (AGENTS.md §11.5). It is not known whether Node accepts the cache files in the copy's `resources/app/node-compile-cache/` when the path changes. Hypothesis: if the cache is keyed by file path, the copy misses it.
- **Impact:** a copy that misses the cache starts more slowly than the App did in place in the 2026-11-16 startup runs (`…-startup-bench`), so the two would not be comparable.
- **Approach:** run `startup_bench.cjs` against a verified copy and compare it with the 2026-11-16 run. Then list the files the run added to the copy's cache folder. **1 h.**

## G15. Which App-side event marks a presented frame?

- **Missing:** the App-side signal that marks two things:
  - the first frame at startup (ADR 0024, "startup to first frame"). The QA startup benchmark measures "workbench ready" instead (README, "Tests");
  - the frame that shows a typed key (ADR 0025). The decision names "a CDP frame trace event" but not which one.
- **Impact:** without it, the App side of the typing-latency and first-frame scenarios has no end point. *Update 2026-11-19:* the startup half is bracketed, not resolved (G22); the typing half waits for ADR 0028. *Update 2026-11-23:* the startup half has an event, the end of the first `Display::DrawAndSwap` after the first line is in the DOM (G22, [finding 0004](../09-findings/0004-full-app-suite.md)); the typing half still waits.
- **Approach:** record a CDP trace (`Tracing.start`) while keys are injected on the virtual display, and match the frame events against screenshots. **2–4 h.**

## G22. Which first-frame events make the App comparison like for like?

- **Missing:** the App has no event that equals rustode's "the first wgpu present returned" (G15's question, for startup). The first comparison ([`09-findings/0003`](../09-findings/0003-first-app-comparison.md)) brackets it: rustode's `first frame presented` line against the App's Chromium first contentful paint (read over CDP as a wall-clock time) and the first fixture line seen in the DOM (polled every 10 ms). The three are not the same event, so no ratio is claimed.
- **Update 2026-11-23 (ticket 42, [finding 0004](../09-findings/0004-full-app-suite.md)):** the prototype works. With `trace = true` each App launch records a Chromium trace (categories `viz` and `blink.user_timing`), a `MutationObserver` marks the first line entering the DOM, and the end of the first `Display::DrawAndSwap` after the mark is the App's first presented frame (`bench/launch.cjs`, `finishTrace`); the trace and `process.hrtime` share a clock. The report now has "launch to first swap" and "launch to second swap" next to rustode's "launch to first frame", and a control run without the trace bounds its cost (about 37 ms at p50 in the one pair of runs).
- **Still missing:** a check that the first swap already shows the first line (the second swap bounds it), the trace's cost on more than one pair of runs, and the same event for a typed key, which waits for [ADR 0028](adr/0028-virtual-display-input.md).
- **Approach:** compare each swap with a screencast frame (`Page.startScreencast`, which carries timestamps) and repeat the control pair three times. **0.5 day.**

## G23. The QA harness's App runs may have used the GPU

- **Missing:** whether the 2026-11-13 and 2026-11-16 QA runs rendered in software. Finding 0003 measured that, without extra environment, the App on `run_virtual.sh`'s display chose the machine's NVIDIA GPU (`SystemInfo.getInfo`: `gpu_compositing: enabled`, ANGLE on `NVIDIA GeForce RTX 3050`), although `run_virtual.sh` states that the Chromium there renders in software, and ADR 0006 says so too. The benchmark runner now sets Mesa's llvmpipe for both editors (`rustode bench`, `plan.rs: software_rendering_env`). The QA runs' `app.log` and `sampler.jsonl` do not record the GPU, so their rendering is a hypothesis.
- **Approach:** re-run one QA startup benchmark with the software-rendering environment and compare with the `…-startup-bench` figures in the ROADMAP. **0.5 day.**

## G24. `rustode-gui` exits with code 0 and no message when it finds no graphics adapter

- **Missing:** a GUI that cannot create its wgpu adapter logs the error through the `log` crate, which `rustode-trace` did not forward, and exits with status 0 (eframe turns the error into an exit of the event loop). Found while building the benchmark runner: on the virtual display with the machine's default EGL driver, `rustode-gui` returned after 70 ms with nothing on stderr. `tracing-subscriber`'s `tracing-log` feature is now on, so `RUSTODE_LOG=info` prints eframe's "Exiting because of error: WGPU error: No suitable graphics adapter found". The exit status is still 0.
- **Closed 2026-11-23 (ticket 42):** `rustode_gui::run` returns an error when the event loop ends before any frame was presented, so `main` prints it and exits with 1 (`crates/gui/src/run.rs`, `ended`; test `a_loop_that_ended_without_a_frame_is_an_error_with_a_hint`). Measured on the virtual display with the machine's default EGL driver: `error: app creation error: the window closed before it presented a frame: no usable graphics adapter (run with RUSTODE_LOG=info for the graphics error)`, exit status 1 ([finding 0004](../09-findings/0004-full-app-suite.md), `0004-attachments/g24-check.txt`).

## G25. The edit scenario does not separate the App's model from its view

- **Missing:** `edit-undo-10k` applies 10k single inserts through the App's `TextModel` with the editor open, and through rustode's `Workspace::replace`; the App takes about 9.3 s and rustode about 14 ms ([finding 0004](../09-findings/0004-full-app-suite.md)). Each App insert fires the content-change events of the open editor, minimap, bracket matching and tokenization, and takes a line and column from `getPositionAt`; rustode's GUI redraws once after the script. How much of the App's time is the model and how much the view is unknown (hypothesis: mostly the view and its listeners, 0.9 ms per insert).
- **Approach:** run the same seeded edits on a `TextModel` with no editor attached (a second model created from the same text through the model service), and as one batched `pushEditOperations` of 10k operations against rustode's `replace_ranges` (one undo step); report both next to the current rows. **0.5 day.** Typing latency stays unmeasured until the owner accepts [ADR 0028](adr/0028-virtual-display-input.md).

## G26. The search scenario is warm, small and has a coarse clock for rustode

- **Missing:** `search-monorepo` runs on a generated tree of 5,626 files (91.8 MB) that is in the page cache, with one regex; rustode's first result and its end fall in the same presented frame (96.7 ms both at p50), so its streaming latency is not seen. A cold-cache search, a tree ten times larger, a plain-text query and a query that hits the 20,000-result limit are not measured, and ripgrep's own options in the App (`search.useRipgrep`, `search.followSymlinks`) are at their defaults.
- **Approach:** add `cache = "cold"` to the scenario (the eviction helper already drops a folder's files), a second list entry with 50,000 files, and an in-process `Workspace::start_search` timing with the first poll's own clock for rustode. **0.5 day.**

## G16. The App lost the first injected keystroke once

- **Missing:** the cause. In one of 3 App sessions through `org_kde_kwin_fake_input`, the first key (Shift+H) never reached the renderer ([`09-findings/0002`](../09-findings/0002-virtual-wayland-input.md), run 2). The finding's hypothesis is a race between the window getting focus or keymap and the first event from a newly created device. One case doesn't make a rule.
- **Impact:** a typing-latency run that loses keys times fewer events than it sent.
- **Approach:** repeat with a warm-up key, through both paths, and check focus, keymap and device creation. **1–2 h.**

## G17. The TUI layout from 60 to 79 columns is undefined

- **Missing:** [ADR 0016](adr/0016-tui-ratatui.md) defines four width bands: sidebar, editor and status bar at 120 columns or more; sidebar collapsed from 80 to 119; editor only below 60; a "terminal too small" notice below 40×10. Nothing says what 60 to 79 columns shows.
- **Current choice (provisional):** the TUI tracer (`a8c6252`) treats 60 to 79 columns like 80 to 119: the status bar and no sidebar, unless the explorer has focus (`crates/tui/src/view.rs:32-68`; snapshot `crates/tui/tests/snapshots/narrow-70x12.txt`).
- **Approach:** **owner decision:** confirm this choice or set another one.

## G18. Dark Modern or the App's default theme, and may theme colors be reused?

- **Missing:** two answers.
  - Phase 1 chose Dark Modern ([ADR 0016](adr/0016-tui-ratatui.md), [ADR 0017](adr/0017-gui-egui-wgpu.md)). The App's default dark theme is "Dark 2026" (`src/vs/workbench/services/themes/common/workbenchThemeService.ts:43`; desktop default of `workbench.colorTheme` at `src/vs/workbench/services/themes/common/themeConfiguration.ts:40`). Dark Modern still ships (`extensions/theme-defaults/package.json:35-38`). Should rustode keep Dark Modern or follow the App?
  - The colors live in data files (`extensions/theme-defaults/themes/dark_modern.json`, `2026-dark.json`, MIT). [ADR 0009](adr/0009-mit-and-reimplemented-behavior.md) forbids copying VS Code code. Does taking color values from these files count as copying?
- **Current choice (provisional):** the TUI draws in Dark Modern (`818073e`), as the Phase 1 specification asks (story 70) and ADR 0016 records. Its color values are taken from `extensions/theme-defaults/themes/dark_modern.json` and from the color registry's defaults, and are credited in [`ATTRIBUTION.md`](../ATTRIBUTION.md) (`crates/tui/src/theme.rs`). The App's default stays "Dark 2026".
- **Approach:** **owner decision** on both: confirm Dark Modern or switch to the App's default, and confirm that taking the color values is not copying code.

## G19. Is branch protection available for the private repository?

- **Missing:** whether the owner's plan offers server-side protection of `main` for a private repository ([ADR 0022](adr/0022-commits-branches-pull-requests.md)). Nothing can be checked yet: the repository has not been pushed (measured on 2026-11-17: `git status -sb` shows `## main` with no upstream).
- **Approach:** after the first push (**owner decision**), read the branch settings on the remote, for example with `gh api repos/<owner>/<repo>/branches/main/protection`. If protection is not available, ADR 0022 records "no push to main" as a convention, enforced by the pre-push hook and CI only. **15 min.**

## G20. Replace across files: how far it is from VS Code

- **Missing:** four differences from the Search view's replace, found while building ticket 34.
  - The preview is the match's line before and after (a struck-through match followed by its replacement), not VS Code's diff editor per file (`src/vs/workbench/contrib/search/browser/replaceService.ts:140-166`).
  - VS Code edits a closed file through a hidden model with no tab, and keeps the bulk edit's undo in its own stack (`replaceService.ts:115-138`). Documents in rustode always have a tab, so each edited file stays open as a tab, and Ctrl+Z works in it; `rustode.search.undoReplace` undoes every file of the last replace at once, only while none was edited since.
  - VS Code saves every edited file after the replace (`replaceService.ts:133`), including the other unsaved edits of a file that was already dirty. **Owner decision, 2026-11-19:** rustode saves only the files that were clean before the replace (closed files it opened count as clean). A file with unsaved changes keeps the replacement in memory and stays dirty, and the notice counts such files. Closed as a gap: a deliberate difference.
  - The preview of a file larger than 64 MiB is not built, and such a file is refused when replacing (`crates/workspace/src/search/replace.rs`).
- **Approach:** the third point is decided (see above); the others need a tab-less document model and a diff view (M6/M7). **1–2 days.**

## G21. Editor groups: how far they are from VS Code

- **Missing:** differences from VS Code's editor groups, found while building ticket 35 ([ADR 0031](adr/0031-editor-groups.md)).
  - Dividers do not drag and groups share a split equally: the workspace keeps no sizes (`Layout` has none). VS Code resizes by dragging and keeps proportions (`src/vs/base/browser/ui/grid/`, `splitview`). Needs sizes in `Layout`, a resize command and a drag in each frontend.
  - In the GUI the group without focus has no minimap, hover or suggest overlays and no find highlights; they belong to the active group's editor.
  - Not registered: `splitEditorLeft`, `splitEditorUp`, `splitEditorOrthogonal`, `moveEditorToLeftGroup` and the other directional moves, the fourth to eighth `focus*EditorGroup`, `lastGroup`, `closeAllGroups`, group locking, maximize, `workbench.editor.closeEmptyGroups` and `workbench.editor.splitInGroupLayout` as settings (empty groups always close), and dragging a tab to another group.
  - The group layout and each group's tabs are not saved between sessions.
  - Scroll is kept per group and document by the frontends, as a top line; it does not follow an edit above it made in another view, as VS Code's per-view scroll anchor is not modelled.
  - In the TUI, Ctrl+1 to Ctrl+3, Ctrl+Alt+arrows and Ctrl+Shift combos reach the program only with an extended keyboard protocol; Ctrl+\ arrives as Ctrl+4 without it. The palette runs every group command in any terminal.
  - Whether VS Code puts a second view's empty cursor after text typed exactly at it (rustode does) was not measured; the studied source was not read for it (hypothesis).
- **Approach:** sizes and dragging first (one ticket, both frontends, about 1 day); the rest are small commands and settings. **1–2 days.**

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-23 | minor | G22: the App's first presented frame from a Chromium trace; G24 closed; G25 and G26: the edit and search scenarios' limits (ticket 42, finding 0004) |
| 2026-11-19 | minor | G21: editor groups against VS Code's |
| 2026-11-19 | minor | G20: replace across files against VS Code's; owner decision: save only clean files |
| 2026-11-19 | minor | G22–G24: first-frame events, the QA harness's GPU, the GUI's silent exit without an adapter |
| 2026-11-17 | minor | G17 and G18: the TUI's provisional layout from 60 to 79 columns and its Dark Modern colors, left for the owner to confirm |
| 2026-11-17 | minor | Editor questions G13–G19 (integrity, benchmark method, TUI layout, theme, branch protection) |
| 2026-11-16 | minor | Translated to English; G1 mentions REA |
| 2026-11-16 | major | Created from the QA, the port and the build tests |
