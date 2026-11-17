# rustode

Reverse engineering of a local build of **VSCodium 1.147.0**, and the home of the **rustode editor**, a Rust editor that reimplements the behavior studied there. The project holds the built app, the exact source code that produced it, the patches, the QA results and the supporting material for reading the packaged JavaScript. The editor lives next to them, in `crates/` and `xtask/`. rustode is not VS Code or VSCodium, and is not affiliated with either.

> [!NOTE]
> Everything in the folders `01-app/` to `08-testes-do-build/` is a **real, read-only copy**, rebuilt by [`montar.sh`](montar.sh) from the live sources. Analyses go in [`09-analises/`](09-analises/) and reference documents in [`docs/`](docs/). The rules are in [`AGENTS.md`](AGENTS.md); the editor's are in its §11.

[What it is](#what-it-is) · [Findings](#findings) · [Tests](#tests) · [Patches](#patches) · [Architecture](#architecture-of-the-analyzed-app) · [The editor](#the-editor) · [Layout](#layout) · [Reverse engineering](#reverse-engineering) · [Reproduce](#reproduce) · [Documentation](#documentation) · [Status](#status) · [License](#license-and-credits)

## What it is

- **The app:** VS Code at commit `22d8dccf0f29c4c824501c342e4d3c8a69776214` (version 1.147.0 of `main`, in development). It carries the VSCodium `ca7f7b44b5e5` patches, ported from 1.143.1. It runs on Electron 43.8.6, Chromium 150.0.7871.250 and Node 24.22.1, as a `codium` binary for Linux x64.
- **Why study it:** it is the most direct way to see, in the assembled product, what VSCodium changes in VS Code: branding, telemetry, gallery, and AI turned off. It also shows how VS Code organizes processes, bundles and dependencies.
- **What makes the analysis feasible:** the build's exact source tree is here (`02-engenharia-reversa/fonte/arvore/`), together with the full diff against the original VS Code. The bundle has no source maps ([ADR 0001](docs/adr/0001-bundle-sem-source-maps.md)), so the source tree is the reference ([ADR 0004](docs/adr/0004-arvore-fonte-e-a-referencia.md)).

## Findings

The findings so far came from QA and from the build. Details and evidence are in [`docs/qa.md`](docs/qa.md) and [`docs/lacunas.md`](docs/lacunas.md).

| Finding | Evidence |
|---|---|
| No Microsoft service is contacted. The only hosts are Open VSX, the Eclipse Foundation's malicious-extension list (GitHub) and Electron's spell-checker dictionary (Google) | [`09-analises/0001-trafego-de-rede.md`](09-analises/0001-trafego-de-rede.md) |
| AI ships turned off (`chat.disableAIFeatures: true`), which also hides the MCP commands | patch `00-copilot-fix-action-condition`; runs `…-pendencias` and `…-pendencias-ia` |
| Without a recorded commit, the agent host looks for modules in the wrong place (`node_modules` instead of `node_modules.asar.unpacked`) | [`docs/aprendizado/0001-commit-muda-caminho-dos-modulos.md`](docs/aprendizado/0001-commit-muda-caminho-dos-modulos.md) |
| The first time a folder is opened, the secondary sidebar shows up empty | `docs/lacunas.md` G7 |
| SIGTERM to all processes at once leaves the app stuck (inherited from VS Code) | `docs/lacunas.md` G8 |
| Keystrokes can be injected into the app on the virtual display through libei or `org_kde_kwin_fake_input`; `zwp_virtual_keyboard_v1` is not offered | [`09-analises/0002-entrada-virtual-wayland.md`](09-analises/0002-entrada-virtual-wayland.md) |

## Tests

Summary of the 2026-11-16 runs on the built app. The reference is Arch's `code` 1.138, run on the same virtual display. Each run is in [`08-testes-do-build/`](08-testes-do-build/RESUMO.md).

| Test | VSCodium 1.147 build | 1.138 reference |
|---|---|---|
| Static smoke | 0 failures | not applicable |
| Network and privacy | Passed; 0 connections outside the proxy | Passed |
| Startup (workbench ready, 10 repetitions) | p50 1905 ms · p90 2201 ms | p50 2232 ms · p90 2499 ms |
| Menus · debugging · integrations · recovery · offline | 11/11 · 9/9 · 10 + 1 blocked · 3 + 1 caveat · 4 + 1 caveat | the same |
| Pending items | 4/5 with the defaults; 5/5 with AI turned on | 5/5 |
| Load: search across 5000 files · open 30 files (p90) | 104–132 ms · 353 ms | 104–168 ms · 383 ms |

## Patches

55 patches were applied:
- 32 VSCodium patches unchanged;
- 19 VSCodium patches adapted to 1.147;
- 4 patches exclusive to the port (96 to 99).

Another 6 were skipped because they only serve architectures other than x64. The full table is in [`docs/patches.md`](docs/patches.md), and the reason for each adaptation is in [`06-port/PORT-NOTES.md`](06-port/PORT-NOTES.md).

## Architecture of the analyzed app

| Process | Bundle in `resources/app/out/` | Role |
|---|---|---|
| Main (Electron main) | `main.js` → `mainImpl.js` | windows, lifecycle, IPC, native services |
| Workbench window (renderer) | `vs/workbench/workbench.desktop.main.js` (21.4 MB) | the whole editor UI |
| Sessions window (renderer) | `vs/sessions/sessions.desktop.main.js` (23.7 MB) | agent sessions window |
| Shared process (utility) | `vs/code/electron-utility/sharedProcess/sharedProcessMain.js` | extensions, storage, background tasks |
| Extension host (Node) | `vs/workbench/api/node/extensionHostProcess.js` | runs the extensions |
| Terminal host (utility) | `vs/platform/terminal/node/ptyHostMain.js` | pseudoterminals |
| Agent host (utility) | `vs/platform/agentHost/node/agentHostMain.js` | agents; here without Copilot or providers |
| CLI | `cli.js` → `vs/code/node/cliProcessMain.js` | `codium --version`, `--install-extension` |

The detail, with layers, workers and the path of each file in the source tree, is in [`docs/arquitetura.md`](docs/arquitetura.md).

## The editor

rustode is a Rust editor with one UI-agnostic core and thin frontends: a CLI, a terminal UI (ratatui) and a GUI (egui). It reimplements behavior studied in the source tree, credits it in [`ATTRIBUTION.md`](ATTRIBUTION.md), and is measured against the app on the same virtual display. Phase 1 has ten milestones, M0 to M9 ([ADR 0018](docs/adr/0018-phase-1-milestones.md)).

- **Today (`d6debfb`):** M1, the core and the CLI. Open and save keep the encoding and line endings; editing with undo grouped like VS Code; multi-cursor; reload or conflict when a file changes on disk; `settings.json` and `keybindings.json` in VS Code's format. The TUI, the GUI and the comparison with the app are not started.
- **Numbers (measured on 2026-11-17):** 133 tests pass (`cargo nextest run --workspace --locked --offline -j 4`); line coverage is 95.1% on the core crates (`cargo xtask coverage`); opening a 1 GB file takes 2.29 s in a release build (`crates/workspace/tests/stress.rs:102-103`). The commands and the other numbers are in [`ROADMAP.md`](ROADMAP.md), "Key numbers".
- **Where to read more:** [`AGENTS.md`](AGENTS.md) §11 (rules), [`CONTRIBUTING.md`](CONTRIBUTING.md) (conventions and review checklist), [`docs/adr/`](docs/adr/) 0007–0028 (decisions), and [`ROADMAP.md`](ROADMAP.md) (milestones and the Gap Analysis against VS Code).

## Layout

| Folder | Contents |
|---|---|
| `01-app/VSCode-linux-x64/` | **the build target**: the packaged app (`codium` binary, and `resources/app` with `out/`, `node_modules.asar`, 93 extensions and the Node compile cache) |
| `02-engenharia-reversa/` | supporting material ([below](#reverse-engineering)) |
| `03-documentos/` | QA documents, port notes, VSCodium documentation and network reports |
| `04-fotos/` | every test screenshot, in the same folder tree as the results |
| `05-qa/` | the full QA project: `docs/`, `suite/`, `results/` and the git history |
| `06-port/` | port scripts, original (`patches/orig/`) and adapted (`patches/port/`) patches, and the logs of each run |
| `07-vscodium-origem/` | the VSCodium the port started from: patches, docs, scripts and `product.json` |
| `08-testes-do-build/` | test runs on the built app and the comparisons (from 2026-11-16 on), with `RESUMO.md` |
| `09-analises/` | **this project's analyses** (numbered findings; not touched by `montar.sh`) |
| `docs/` | reference documents: architecture, build, QA, gaps, ADRs, templates |
| `crates/` · `xtask/` · `Cargo.toml` · `Cargo.lock` | **the rustode editor**: one crate per module ([`AGENTS.md`](AGENTS.md) §11.1) and the project task runner (`cargo xtask`) |
| `.github/` · `.pre-commit-config.yaml` · `.semgrep/` · `deny.toml` · `committed.toml` · `clippy.toml` · `rustfmt.toml` · `rust-toolchain.toml` · `.config/` · `.cargo/` | the editor's quality pipeline: CI, Dependabot and the pull request template; the git hooks; security rules; license policy; commit conventions; lints; formatting; the pinned toolchain; the test profiles; the `xtask` alias |
| `.claude/skills/` · `.opencode/skills/` · `.serena/` · `.ai-memory.toml` | the project's skills (original in `.claude`, identical copy for OpenCode made by `sincronizar-skills.sh`), Serena configuration (TypeScript on the source tree, read-only) and the ai-memory scope |
| `.mcp.json` · `opencode.json` | REA MCP registration (reverse engineering of the packaged app) for Claude Code and OpenCode; rules in [`AGENTS.md`](AGENTS.md) §6 |

Every file in 01 to 08 is a real, independent copy, with no symbolic or hard links. The only link in the VS Code repository, a test fixture in `extensions/terminal-suggest/src/test/fixtures/symlink-test/`, was copied as a regular file.

## Reverse engineering

| Item (in `02-engenharia-reversa/`) | What it is |
|---|---|
| `fonte/arvore/` | **the exact source code that produced the binary**: base commit + patches + post-install steps, without `node_modules` or build outputs (14,465 files) |
| `fonte/port-aplicado.diff` | the full difference between the original VS Code and the build tree; removed files appear only as removed |
| `fonte/origem.txt` | VS Code commit, VSCodium commit, app version, Electron, build git tree |
| `asar/node_modules.asar.extraido/` | runtime dependencies extracted from `node_modules.asar` (the native modules live in `01-app/…/node_modules.asar.unpacked/`); `asar/node_modules.asar.lista.txt` lists the contents |
| `out-formatado/` | the 32 JavaScript files of `resources/app/out`, reformatted by esbuild (names stay minified) |
| `manifesto-sha256.txt` | SHA-256 of the app's 3,008 files, for checking integrity and comparing builds |
| `binarios.txt` | the type of each binary and library (`file`) |
| `product.json`, `package.json`, `versao.txt` | the product's identity and configuration as packaged |

### Peculiarities of this build

- **No source maps in the main bundles** ([ADR 0001](docs/adr/0001-bundle-sem-source-maps.md)). Only 5 `.map` files remain, from small separate builds (preloads and the agent host's crypto module).
- **Node compile cache** generated for UID 1000, because Node puts the UID in the folder name. Under another user the app runs without this cache.
- **Differences from official VSCodium:**
  - data in `.vscodium` and `.vscodium-shared` ([ADR 0002](docs/adr/0002-dados-separados-do-code-oss.md));
  - no update check ([ADR 0003](docs/adr/0003-sem-checagem-de-atualizacao.md));
  - the agent host's restricted telemetry emptied out;
  - patches 96 to 99.

## Reproduce

The build was made by the scripts in `06-port/` (originals in `~/Projects/vscodium-port/`), in this order: `apply.sh` → `install.sh` → `finish.sh` → `build.sh`. The machine has 7.5 GiB of RAM, so the heavy steps run under a memory cap, with a guard that stops the build if it gets stuck swapping. The step-by-step, the environment variables and the pitfalls found are in [`docs/build.md`](docs/build.md).

## Documentation

| | |
|---|---|
| [`AGENTS.md`](AGENTS.md) | inviolable rules and working instructions for agents (`CLAUDE.md` only imports it) |
| [`ROADMAP.md`](ROADMAP.md) | stage, numbers, the editor's milestones and Gap Analysis, next steps, progress log |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | how to contribute; the editor's commit, branch and check conventions, and the code review checklist |
| [`docs/arquitetura.md`](docs/arquitetura.md) · [`docs/fonte-vs-pacote.md`](docs/fonte-vs-pacote.md) | processes and layers · origin of each part of the package |
| [`docs/build.md`](docs/build.md) · [`docs/patches.md`](docs/patches.md) | how the app was built · all the patches |
| [`docs/qa.md`](docs/qa.md) · [`docs/lacunas.md`](docs/lacunas.md) | test results and criteria · open questions |
| [`docs/adr/`](docs/adr/) · [`docs/glossario.md`](docs/glossario.md) | decisions · project terms |
| [`docs/modelos/`](docs/modelos/) · [`docs/aprendizado/`](docs/aprendizado/) | document templates · reusable lessons |
| [`ATTRIBUTION.md`](ATTRIBUTION.md) · [`NOTICE`](NOTICE) | provenance and licenses of the material |

## Status

**Study stage: preparation.** The app is built and tested, the supporting material is assembled and the project documents exist; the analysis proper starts now. Numbers measured on 2026-11-16, and on 2026-11-17 for the analyses:

- **Test coverage:** 15 runs on the built app and the comparisons. All passed except for failures already explained: a harness defect, a VSCodium default and an unsuitable criterion.
- **Recorded analyses:** 2 (`09-analises/0001`, `0002`).

**Editor stage: Phase 1, M1 (core and CLI).** M0 waits for the owner's first push; M2 to M9 are not started ([`ROADMAP.md`](ROADMAP.md), "Editor: Phase 1 milestones").

The biggest gaps today ([`docs/lacunas.md`](docs/lacunas.md)):

- No map from each bundle to the source-tree files (G1).
- The 2 h soak has not been run (G4).
- The `product.json` list of proposed APIs has not been checked against 1.147 (G6).
- For the editor: the app side of a presented frame is not defined yet (G15), and two layout and theme questions wait for the owner (G17, G18).

## License and credits

The analyzed material keeps its licenses. Code - OSS, VSCodium and Electron are MIT; Chromium and the other dependencies use free licenses. See [`NOTICE`](NOTICE) and [`ATTRIBUTION.md`](ATTRIBUTION.md). "Visual Studio Code" and "VS Code" are Microsoft trademarks, and "VSCodium" belongs to the VSCodium project. This project is not affiliated with any of them. The editor code is declared MIT in `Cargo.toml` ([ADR 0009](docs/adr/0009-mit-and-reimplemented-behavior.md)), and its dependencies are listed in `ATTRIBUTION.md`. The `LICENSE` file and the license of the analysis documents are still the owner's decision.

> [!WARNING]
> `05-qa/results/backup-sharedStorage-125048.vscdb` is a backup of the owner's recent files, saved before the cleanup done during QA. It contains personal folder and file paths (AGENTS.md §2.1).
