# Build QA

> **Last reviewed:** 2026-11-16 · **Last updated:** 2026-11-16 · **Change:** minor (translated to English) · **Target:** local VSCodium 1.147.0; reference `code` 1.138.0 (Arch)

This is a summary of the tests on the built app. The full QA document is [`05-qa/docs/06-testes-do-build-vscodium.md`](../05-qa/docs/06-testes-do-build-vscodium.md). Each run is in [`08-build-tests/`](../08-build-tests/SUMMARY.md), with `steps.json` (steps) and `screenshots/`.

## 1. Environment

- **Machine:** Arch Linux, KDE Wayland, 12 cores, 7.5 GiB of RAM, no oomd.
- **Display:** the UI tests run on a virtual display (`kwin_wayland --virtual`, with its own D-Bus) without a GPU, so rendering is in software. That is why the timing comparisons use `code` 1.138 run on the same display.
- **Harness:**
  - the app is driven over CDP (`--remote-debugging-port`);
  - each run gets a fresh profile (`--user-data-dir`, `--extensions-dir`, `--shared-data-dir`);
  - a watchdog aborts below 700 MB free.

## 2. Results

| Test | Run | Result | 1.138 reference |
|---|---|---|---|
| Static smoke | `smoke.sh` | 0 failures | — |
| Network and privacy | `2026-11-16-codium1.147-rede` | Passed; 0 connections outside the proxy; no Microsoft host | Passed |
| Startup, 10 repetitions | `…-codium1.147-startup-bench` | workbench ready p50 1905 ms, p90 2201 ms; SIGTERM p50 299 ms | p50 2232 ms, p90 2499 ms |
| Menus | `…-codium1.147-menus-r2` | 11 Passed, 1 Not run (native About) | the same |
| Debugging | `…-codium1.147-debug` | 9/9 | 9/9 |
| Integrations | `…-codium1.147-integracoes` | 10 Passed, 1 Blocked (account and network) | the same |
| Pending items | `…-pendencias` / `…-pendencias-ia` | 4/5 with the VSCodium default; 5/5 with `chat.disableAIFeatures: false` | 5/5 |
| Recovery | `…-codium1.147-recuperacao` | 3 Passed, 1 with caveats (G8) | the same |
| Offline (`bwrap --unshare-net`) | `…-codium1.147-sem-rede` | 4 Passed, 1 with caveats (only reads logs) | the same |
| Load | `…-codium1.147-carga` | search 104–132 ms; 30 files p90 353 ms; 3.88 ms/character; terminal 0.9 s; L05 failed on its criterion (G10) | search 104–168 ms; p90 383 ms; 4.49 ms/character |
| Stress | `…-codium1.147-stress` | 75 files without degradation up to the RAM limit; large files were not run (G3) | — |

## 3. Criteria

The criteria marked "(inferred)" in the load, stress and soak scenarios were set without a prior baseline and need review after the first run. L05 has already shown an unsuitable criterion (G10).

| Area | Check | Criterion |
|---|---|---|
| Network | hosts in the logging proxy | no Microsoft telemetry, experiments, Marketplace, sync or Copilot host; 0 connections outside the proxy |
| Startup | spawn → workbench ready | no regression against the reference on the same display |
| Load (inferred) | search, opening, typing, terminal | correct results; search p90 < 5 s; opening p90 < 1.5 s |
| Stress (inferred) | steps up to the soft RAM limit | no hang; UI responsive; recovers on close |
| Soak (inferred) | 2 h of cycles | heap growth < 10 MB/h and listener growth < 5 %/h; < 5 % of cycles with errors |

## 4. Harness errors fixed

| Error | Effect | Fix |
|---|---|---|
| CLI called by a fixed path (`/usr/bin/code`) in two scenarios | the CLI step tested the wrong binary | the CLI of the binary under test (`QA_CLI` or `bin/<name>`) |
| M12 re-read the "View" menu only in the visible bar | false failure on 1.147 | `openPath` goes through the "···" |
| `run_virtual.sh` reopened its output without appending | logs overwritten between runs | `exec >>` |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-16 | minor | Translated to English |
| 2026-11-16 | major | Created from the 2026-11-16 runs |
