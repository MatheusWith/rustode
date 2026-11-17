# ADR 0026: Micro-benchmarks block regressions above 5% locally and 10% in CI

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session); not applied yet (no benchmark and no CI job at `d6debfb`)
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

- Measured at `d6debfb`: no benchmark exists (`find crates xtask -name benches` prints nothing), and `.github/workflows/ci.yml` has no benchmark job.
- Hypothesis: runner noise stays below 10%. It can be checked only once the job runs.
- The gate depends on CI existing on the remote, so it waits for the first push ([ADR 0007](0007-own-git-repository.md)).
