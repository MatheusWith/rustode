# ADR 0012: Phase 1 builds, tests and benchmarks on Linux x64 only

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in `.github/workflows/ci.yml`)
- **Build:** rustode at `d6debfb`; the App: local VSCodium 1.147.0

## Context

The App is a Linux x64 build (README, "What it is"). Everything the comparison depends on is Linux-specific:

- the virtual display is `kwin_wayland --virtual` ([ADR 0006](0006-tests-on-virtual-display.md));
- the keystroke-injection paths are KWin's ([`09-findings/0002`](../../09-findings/0002-virtual-wayland-input.md));
- the QA harness reads `/proc/meminfo` for its memory guard (`05-qa/suite/scenarios/run_scenario.sh:26-28`).

Windows and macOS builds would multiply the CI and benchmark work, and there is no baseline on those systems to compare against.

## Decision

Phase 1 targets Linux x64 only, locally and in CI.

Containment rules:

1. Every CI job runs on `ubuntu-latest` (`.github/workflows/ci.yml:22`, `:39`, `:51`, `:69`).
2. There are no Windows or macOS builds, CI jobs or benchmarks in Phase 1.
3. Tests that read Linux interfaces say so. For example, the stress tests read `/proc/meminfo` and `/proc/self/status` (`crates/workspace/tests/stress.rs:43-63`).

## The guarantee

Every claim about the editor's behavior or performance refers to Linux x64.

## Consequences

- Code that is portable through its crates (notify, encoding_rs, ropey) is still not proven portable. For example, file watching uses notify's `RecommendedWatcher` (`crates/fs/src/watch.rs:10`, `:69`), which is inotify on Linux and a different backend elsewhere (notify 8.2.2, `src/lib.rs:408-414`, in the local cargo registry). Only the inotify one has been tested.
- Supporting another platform needs a new decision (Phase 2 or later).
