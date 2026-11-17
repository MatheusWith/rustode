# ADR 0020: Line coverage is at least 85% on the core crates and 60% on the frontends, and the thresholds are never lowered

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in `xtask/src/coverage.rs:16-35` and in the `coverage` CI job, `.github/workflows/ci.yml:38-48`)
- **Build:** rustode at `d6debfb`

## Context

Test-first work ([ADR 0019](0019-tests-at-three-seams.md)) can be checked only if coverage is measured. The frontends hold more glue and presentation code, which seam tests reach less, so they get a lower bar.

## Decision

`cargo xtask coverage` runs the instrumented test suite. It fails in two cases:

- line coverage below 85% on `core`, `syntax`, `fs`, `config`, `workspace`, `protocol` and `trace`;
- line coverage below 60% on `cli`, `tui` and `gui`.

Containment rules:

1. The groups and the thresholds live only in `xtask/src/coverage.rs:16-35`.
2. An exception is justified in a comment on the pull request. The thresholds are never lowered.
3. A group with no instrumented lines yet passes, with a note (`xtask/src/coverage.rs:60-72`).
4. `xtask` is excluded from the measurement (`xtask/src/coverage.rs:96-97`).

## The guarantee

Coverage cannot erode silently: a drop below a threshold fails CI.

## Consequences

- Measured at `d6debfb` on 2026-11-17: core 95.1% of 3,482 lines, frontends 85.7% of 21 lines. Command, from the project root: `LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo xtask coverage`.
- Until M2, the frontends gate covers almost nothing: 21 lines, while the TUI and GUI crates hold only a doc comment.
- Line coverage does not show that the assertions are meaningful. The review checklist covers that ([`CONTRIBUTING.md`](../../CONTRIBUTING.md)).
