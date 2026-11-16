# How the app was built

> **Last reviewed:** 2026-11-16 · **Last updated:** 2026-11-16 · **Change:** minor (translated to English) · **Target:** local VSCodium 1.147.0

This is a summary of the build process.
- The reason for each adaptation is in [`06-port/PORT-NOTES.md`](../06-port/PORT-NOTES.md), and the logs of each run are in `06-port/logs/`.
- The original scripts live in `~/Projects/vscodium-port/`.
- The build tree is `~/Projects/vscode-vscodium`, a worktree of the VS Code repository; the main checkout was not changed.

## 1. Order

| Step | Script | What it does |
|---|---|---|
| 1 | `apply.sh` | cleans the worktree, then applies, in order: `src/stable`, `LICENSE`, the `product.json` (VSCodium identity, Open VSX, `.vscodium`, no `updateUrl`), the JSON actions and the patches (`patches/port/<name>` if it exists, otherwise `patches/orig/`). Last come the port-only patches (`9*-port-*.patch`) |
| 2 | `install.sh` | `pnpm install --no-frozen-lockfile` under a memory cap (the patches change dependencies) |
| 3 | `finish.sh` | the post-install steps of `prepare_vscode.sh`: version and author, gulp heap, built-in announcements, telemetry redirection (`*.data.microsoft.com` → `0.0.0.0`, including `node_modules/.pnpm`), Linux metadata |
| 4 | `build.sh` | `gulp vscode-min-prepack` (extensions), policy definitions, `gulp vscode-linux-x64-min-packing` (bundle, assembly, compile cache) |
| — | `smoke.sh` | static check of the built app (identity, telemetry, Copilot, native forks) |

`apply.sh` on a clean worktree reproduces exactly the validated tree (measured: the same git tree `cdad87a` before and after reapplying and reinstalling).

## 2. Environment variables that matter

| Variable | Where | Why |
|---|---|---|
| `VSCODE_BUNDLE_SOURCEMAPS=0` | `build.sh` (patch 97) | without source maps, the bundle fits in RAM (ADR 0001) |
| `VSCODE_BUNDLE_SEQUENTIAL=1` | `build.sh` (patch 98) | one entry point at a time in esbuild |
| `BUILD_SOURCEVERSION=<commit>` | `build.sh` | in a worktree the build can't find the commit on its own (`.git` is a file) |
| `MAX_OLD_SPACE_SIZE=2560` | `finish.sh`, `build.sh` | gulp heap below the cgroup's hard cap |
| `ELECTRON_SKIP_BINARY_DOWNLOAD=1`, `npm_config_build_from_source_native_keymap=no` | `install.sh` | install without needlessly downloading Electron; prebuilt keymap |

## 3. Pitfalls found

| Symptom | Cause | Fix |
|---|---|---|
| Global OOM during the bundle (2026-11-16, 08:29) | esbuild with all entry points in parallel | patch 98 (sequential), which was not enough on its own |
| Bundle stuck swapping for 30 min, memory pressure ~85% | the bundler's Node process keeps all outputs and source maps (with `sourcesContent`) until the end | patch 97 (no maps) + a swap guard in `lib.sh` |
| "Missing X server or $DISPLAY" at the end of packaging | generating the compile cache opens the app | run the packaging on a virtual display (`kwin_wayland --virtual`) |
| "produced 0 x64 version-tag directories" | Node puts the UID in the cache folder name, which the build's validation did not accept | patch 96 |
| `--version` shows "Unknown commit"; the agent host can't find `tree-sitter.wasm` | in a worktree the build doesn't read the commit | `BUILD_SOURCEVERSION` |
| Telemetry redirected in only 1 file | with pnpm the packages live in `node_modules/.pnpm`, hidden from `rg` | `--hidden` limited to `.pnpm` in `finish.sh` (105 files) |

## 4. Reproduce

```sh
cd ~/Projects/vscodium-port
./apply.sh && ./install.sh && MAX_OLD_SPACE_SIZE=2560 ./finish.sh
~/Projects/vscode/qa/suite/tools/run_virtual.sh ./build.sh   # virtual display: the compile cache opens the app
./smoke.sh
cd ~/Projects/rustode && ./montar.sh                         # refreshes this project's copies
```

Requirements measured on this machine:
- about 3 GB of free RAM for the bundle (2.7 GB peak in the cgroup);
- 92 GB of free disk;
- Node 24.22.1 in `~/.local/opt/`.

Time: about 2 minutes of packaging after the install (measured: 102 s in the last run).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-16 | minor | Translated to English |
| 2026-11-16 | major | Created from PORT-NOTES and the build logs |
