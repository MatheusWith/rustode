# ADR 0005: Folders 01 to 08 are read-only evidence; analyses go in `09-findings/`

- **Status:** proposed (2026-11-16, created with the project documents; awaiting the owner)
- **Build:** local VSCodium 1.147.0

## Context

`assemble.sh` rebuilds the copies from the live sources:
- `rm -rf` in `03-documents/`, `04-screenshots/` and `08-build-tests/`;
- `rsync --delete` in `01-app/`, `05-qa/`, `06-port/`, `07-vscodium-source/`, the source tree and the extracted asar.

Any file written inside them disappears on the next run, and any change makes the copy stop matching the app.

## Decision

The folders `01-app/` to `08-build-tests/` are read-only. Analyses go in `09-findings/`, reference documents in `docs/`, and temporary work outside the project.

Containment rules:

1. `assemble.sh` does not touch `09-findings/`, `docs/`, the root files or the configuration folder `.claude/`. The only exception is `.opencode/skills/`, which it rebuilds as a copy of `.claude/skills/` (`sync-skills.sh`).
2. The contents of the evidence folders change only in the live sources, followed by `./assemble.sh`.

## The guarantee

What is in 01 to 08 always matches the live sources as of the last assembly, and no analysis is lost when reassembling.

## Consequences

- The matching rule is in [`AGENTS.md`](../../AGENTS.md) §0.1–0.2.
- To note something about an evidence file, the note goes in `09-findings/`, citing the path.
