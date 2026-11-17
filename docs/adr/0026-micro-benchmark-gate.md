# ADR 0026: Micro-benchmarks block regressions above 5% locally and 10% in CI

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session); applied in `crates/core/benches/core.rs`, `xtask/src/bench_gate.rs` (`cargo xtask bench-gate`) and the `bench-gate` job of `.github/workflows/ci.yml`; the CI job has not run yet
- **Build:** rustode at `d6debfb`

## Context

The comparison with the App runs once per milestone ([ADR 0024](0024-comparison-with-the-app.md)), so a regression in the buffer or in undo between two milestones would go unnoticed. CI runners are noisy, so their bar has to be looser than the local one.

## Decision

Each crate has criterion micro-benchmarks, starting with buffer, edit and undo operations. A regression of more than 5% against `main` fails locally, and a regression of more than 10% fails in CI. The comparison against VS Code is never a per-pull-request gate.

Containment rules:

1. The baseline is `main`, measured on the same machine as the candidate (locally) or on the same runner type (in CI).
2. The gate and the way to run it are documented in [`CONTRIBUTING.md`](../../CONTRIBUTING.md) when it lands.
3. criterion is added as a dependency under [ADR 0010](0010-ecosystem-crates-allowed.md): licenses checked by `cargo deny`, and listed in `ATTRIBUTION.md`.

## The guarantee

No merge can make a benchmarked core operation more than 10% slower than `main` without failing CI.

## Consequences

- Applied: nine criterion benchmarks of `rustode-core`, in the groups `buffer`, `edit` and `undo` (`crates/core/benches/core.rs`), and `cargo xtask bench-gate` ([`CONTRIBUTING.md`](../../CONTRIBUTING.md), "Micro-benchmark gate"). CI runs the gate on every pull request at 10% against the base commit. The pre-push hook runs it at 5% only when `RUSTODE_BENCH_GATE=1` is set (`.pre-commit-config.yaml`), because it takes minutes.
- How the gate limits noise, from what the first local runs showed:
  - it builds both trees before measuring either, and then runs the built benchmarks directly, so that no compile, cargo lock wait or fingerprint check falls between two measurements;
  - the base builds into its own target directory, because the two trees' artifacts have the same names and would overwrite each other;
  - it measures in alternating rounds (2 by default) and keeps each benchmark's fastest mean per tree.
- Measured on 2026-11-17, in the bench profile (release, thin LTO, one codegen unit), from the root of a clone at `0f429e8` (branch `feat/micro-benchmarks-gate`), with `cargo xtask bench-gate --base HEAD`, run twice. Both trees were then the same code. The machine (12 threads, 7.5 GiB) was shared with four other agents' builds, at a load average of 7.0 rising to 14.6:
  - one measurement of one tree took 26 to 34 s, and one gate run took 129 and 131 s, including an incremental build of the base (14 s);
  - the largest slowdown was +19.9% (`buffer/from_text_1mib`, second run), and the largest change overall was −32.5% (`edit/insert_char_at_random_offset`, first run);
  - each run reported two benchmarks above 5%, so on a loaded machine the gate fails without any change in the code.
- So the local limit of 5% holds only on a quiet machine, and [`CONTRIBUTING.md`](../../CONTRIBUTING.md) says to close builds and other editors first. A run on a quiet machine has not been measured yet.
- Hypothesis: runner noise stays below 10%. It can be checked only once the job runs on GitHub's runners.
- The gate depends on CI existing on the remote, so it waits for the first push ([ADR 0007](0007-own-git-repository.md)).
