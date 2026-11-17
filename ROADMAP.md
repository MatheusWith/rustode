# rustode: roadmap

**Stage: preparation** · next: active analysis, once there is a bundle → source map of the main processes and the first component analyses ([why](docs/lacunas.md))

> **Last reviewed:** 2026-11-16 · **Last updated:** 2026-11-16 · **Change:** minor (translated to English) · **Target:** local VSCodium 1.147.0 (VS Code `22d8dccf0f29`, VSCodium `ca7f7b44b5e5`, Electron 43.8.6)

This page is the summary. The details are in:
- [`docs/qa.md`](docs/qa.md): tests;
- [`docs/lacunas.md`](docs/lacunas.md): open questions;
- [`docs/patches.md`](docs/patches.md): patches;
- [`09-analises/`](09-analises/): findings.

## Key numbers

| | Value | How |
|---|---|---|
| App built and intact | yes, 3,008 files | measured: `sha256sum -c` against `02-engenharia-reversa/manifesto-sha256.txt` |
| Patches applied | 55 (32 VSCodium unchanged, 19 adapted, 4 port-only); 6 skipped | measured: the `apply.sh` log (`06-port/logs/`) plus patch 96 |
| Test runs on the build | 15, no unexplained failure | measured: `08-testes-do-build/RESUMO.md` |
| Startup (workbench ready, p50) | 1905 ms (1.138: 2232 ms) | measured: `…-startup-bench`, 10 repetitions, virtual display |
| Hosts contacted unprompted | 4 groups, none from Microsoft | measured: `…-codium1.147-rede/network-report.md` |
| Build source tree | 14,465 files | measured: `find 02-engenharia-reversa/fonte/arvore -type f` |
| Recorded analyses | 1 | measured: `ls 09-analises/[0-9]*` |

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

## Next steps

1. **Bundle → source map of the main processes** (G1). For each bundle in `out/`, list its entry points in `fonte/arvore/src` and the modules it includes. 6–10 h.
2. **2 h soak** (G4): the only measurement of leaks over time. 2 h of machine time + 1 h of analysis. **Owner decision:** authorize it and choose the time.
3. **Check the `product.json` list of proposed APIs** (G6) against the 1.147 proposals and VSCodium's newest list. 1–2 h. **Owner decision:** download the newest list from the network.
4. **Stress with more free RAM** (G3): open more than 75 files and test files of 10 to 60 MB. 1 h. **Owner decision:** close other apps or lower the soft limit.
5. **License of the analysis documents.** **Owner decision.**
6. **Index the source tree with CodeGraph.** This makes symbol search much faster, since the tree has about 10 thousand `.ts` files. **Owner decision** (indexing is always the owner's choice).
7. **Review the L05 criterion** (G10): baseline after a warm-up cycle. 30 min.

## Progress log

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
| 2026-11-16 | minor | Translated to English |
| 2026-11-16 | minor | REA registered as a project tool (Claude Code and OpenCode) |
| 2026-11-16 | major | Created from the QA, the port and the build tests |
