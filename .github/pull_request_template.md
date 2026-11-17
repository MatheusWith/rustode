## What and why

<!-- What changes for the user, and why. Link the ticket. -->

## Evidence

<!-- Tests added (which seam), benchmark numbers (measured, with the command, folder and commit), runs. -->

## Review checklist

<!-- The same list as CONTRIBUTING.md, "Code review checklist"; change both together. -->

- [ ] The title is a valid Conventional Commit; it becomes the squash commit on `main`
- [ ] Tests came first, at the highest seam: workspace API, frontend snapshot or CLI process (ADR 0019)
- [ ] Tests assert on behavior, not internals; they are deterministic and use no network, real home directory or real profile
- [ ] Frontends depend on no rustode crate but the workspace, and hold no business logic (ADR 0013, ADR 0014)
- [ ] Behavior studied in VS Code is reimplemented, not translated, and credited in `ATTRIBUTION.md`, the file's doc comment and the component's ADR (ADR 0009)
- [ ] New dependencies pass `cargo deny`, have MIT-compatible licenses and are listed in `ATTRIBUTION.md` (ADR 0010)
- [ ] Coverage gates hold; any exception is justified in a comment on this pull request (ADR 0020)
- [ ] Every lint exception is justified next to its `allow` (ADR 0021)
- [ ] Every number is labeled measured (with its command, folder and commit) or estimated (with its source); no regression beyond the micro-benchmark gate (ADR 0026)
- [ ] Benchmarks against the App ran on a verified temporary copy, one editor instance at a time, under the memory guard (AGENTS.md §11.5)
- [ ] Nothing written to folders 01 to 08; no personal data
- [ ] Docs, ADRs and the roadmap's milestones and Gap Analysis are updated where the change affects them
