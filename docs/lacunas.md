# Gaps and open questions

> **Last reviewed:** 2026-11-16 · **Last updated:** 2026-11-16 · **Change:** minor (translated to English) · **Target:** local VSCodium 1.147.0

This page lists what is still unknown or has not been measured, in priority order. The hour figures are estimates, not measurements. When a gap is resolved:
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

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-16 | minor | Translated to English; G1 mentions REA |
| 2026-11-16 | major | Created from the QA, the port and the build tests |
