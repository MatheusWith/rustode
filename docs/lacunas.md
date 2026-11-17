# Gaps and open questions

> **Last reviewed:** 2026-11-17 · **Last updated:** 2026-11-17 · **Change:** minor (editor questions G13–G19 added) · **Target:** local VSCodium 1.147.0 and the rustode editor, Phase 1

This page lists what is still unknown or has not been measured, in priority order: first the study (G1–G12), then the editor (G13 on). The hour figures are estimates, not measurements. When a gap is resolved:
- record the finding in `09-analises/`;
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
| G17 | The TUI layout from 60 to 79 columns is undefined | specification | the TUI tracer (M2) | — |
| G18 | Dark Modern or the App's default theme, and may theme colors be reused? | specification, license | TUI and GUI colors (M2, M3), token colors (M4) | — |
| G19 | Is branch protection available for the private repository? | repository | no | 15 min |

## G1. No bundle → source-module map

- **Missing:** for each large bundle (`workbench.desktop.main.js`, `sessions.desktop.main.js`, `agentHostMain.js`, `extensionHostProcess.js`), which `src/` modules it contains and in what order. Today only the entry file is known ([`fonte-vs-pacote.md`](fonte-vs-pacote.md) §1).
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
  - Reading `product.json` and `versao.txt`, which already give the same values.

## G6. The `product.json` list of proposed APIs is from 1.143

- **Missing:** whether the current versions of the 65 extensions listed in `extensionEnabledApiProposals` request proposals that are not on the list. Today only the converse is known: 3 of the 305 pairs point to proposals that no longer exist in 1.147 (`agentsWindowActivation`, `timeline`).
- **Approach:** compare with two lists. **1–2 h.**
  - VSCodium's newest list, which has to be downloaded from the network: **owner decision**.
  - `src/vs/platform/extensions/common/extensionsApiProposals.ts`.

## G7. Secondary sidebar empty on first open

- **Missing:** two confirmations.
  - Whether official VSCodium also shows the bar empty.
  - Whether the "it was empty" rule (`AUXILIARYBAR_EMPTY`) hides it even on the second open.
- **Evidence:** the block that tests `ChatAIDisabledSettingId` in `layout.ts` (fonte/arvore/src/vs/workbench/browser/layout.ts); the screenshot `…-codium1.147-menus/screenshots/M12.png`.
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
- **Evidence:** [`09-analises/0001-trafego-de-rede.md`](../09-analises/0001-trafego-de-rede.md).
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

- **Missing:** `sha256sum -c` (§0.6) checks only the files the manifest lists. Files added to `01-app/` go unnoticed. On 2026-11-17, running the App in place wrote 3 new Node compile-cache files under `resources/app/node-compile-cache/extension-host/`, and the check still passed ([`09-analises/0002`](../09-analises/0002-entrada-virtual-wayland.md), "Limitations").
- **Impact:** §0.6 can pass while 01 no longer matches the analyzed app.
- **Evidence today:** the file list matched the manifest on 2026-11-17 (3,008 files). Measured from `01-app/VSCode-linux-x64` with `diff <(find . -type f | sort) <(sed 's/^[0-9a-f]*  //' ../../02-engenharia-reversa/manifesto-sha256.txt | sort)`, which printed nothing.
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
- **Impact:** without it, the App side of the typing-latency and first-frame scenarios has no end point.
- **Approach:** record a CDP trace (`Tracing.start`) while keys are injected on the virtual display, and match the frame events against screenshots. **2–4 h.**

## G16. The App lost the first injected keystroke once

- **Missing:** the cause. In one of 3 App sessions through `org_kde_kwin_fake_input`, the first key (Shift+H) never reached the renderer ([`09-analises/0002`](../09-analises/0002-entrada-virtual-wayland.md), run 2). The finding's hypothesis is a race between the window getting focus or keymap and the first event from a newly created device. One case doesn't make a rule.
- **Impact:** a typing-latency run that loses keys times fewer events than it sent.
- **Approach:** repeat with a warm-up key, through both paths, and check focus, keymap and device creation. **1–2 h.**

## G17. The TUI layout from 60 to 79 columns is undefined

- **Missing:** [ADR 0016](adr/0016-tui-ratatui.md) defines four width bands: sidebar, editor and status bar at 120 columns or more; sidebar collapsed from 80 to 119; editor only below 60; a "terminal too small" notice below 40×10. Nothing says what 60 to 79 columns shows.
- **Approach:** **owner decision**, before the TUI tracer (M2).

## G18. Dark Modern or the App's default theme, and may theme colors be reused?

- **Missing:** two answers.
  - Phase 1 chose Dark Modern ([ADR 0016](adr/0016-tui-ratatui.md), [ADR 0017](adr/0017-gui-egui-wgpu.md)). The App's default dark theme is "Dark 2026" (`src/vs/workbench/services/themes/common/workbenchThemeService.ts:43`; desktop default of `workbench.colorTheme` at `src/vs/workbench/services/themes/common/themeConfiguration.ts:40`). Dark Modern still ships (`extensions/theme-defaults/package.json:35-38`). Should rustode keep Dark Modern or follow the App?
  - The colors live in data files (`extensions/theme-defaults/themes/dark_modern.json`, `2026-dark.json`, MIT). [ADR 0009](adr/0009-mit-and-reimplemented-behavior.md) forbids copying VS Code code. Does taking color values from these files count as copying?
- **Approach:** **owner decision** on both, before the TUI colors (M2).

## G19. Is branch protection available for the private repository?

- **Missing:** whether the owner's plan offers server-side protection of `main` for a private repository ([ADR 0022](adr/0022-commits-branches-pull-requests.md)). Nothing can be checked yet: the repository has not been pushed (measured on 2026-11-17: `git status -sb` shows `## main` with no upstream).
- **Approach:** after the first push (**owner decision**), read the branch settings on the remote, for example with `gh api repos/<owner>/<repo>/branches/main/protection`. If protection is not available, ADR 0022 records "no push to main" as a convention, enforced by the pre-push hook and CI only. **15 min.**

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-17 | minor | Editor questions G13–G19 (integrity, benchmark method, TUI layout, theme, branch protection) |
| 2026-11-16 | minor | Translated to English; G1 mentions REA |
| 2026-11-16 | major | Created from the QA, the port and the build tests |
