## What and why

<!-- What changes for the user, and why. Link the ticket. -->

## Evidence

<!-- Tests added (which seam), benchmark numbers (measured, with the command), runs. -->

## Review checklist

- [ ] The title is a valid Conventional Commit (it becomes the squash commit)
- [ ] Tests were written first, at the highest seam (workspace API, frontend snapshot, or CLI process)
- [ ] Tests assert on behavior, not internals; deterministic; no network, no real profiles
- [ ] Frontends depend only on the workspace crate; no business logic in a frontend
- [ ] No regression beyond the benchmark gate; any number is labeled measured or estimated
- [ ] Coverage thresholds hold; any exception is justified in a comment on this pull request
- [ ] Lint exceptions are justified next to the `allow`
- [ ] New dependencies pass `cargo deny`; licenses are MIT-compatible
- [ ] Behavior studied in VS Code is reimplemented, not translated, and credited in `ATTRIBUTION.md`
- [ ] Nothing written to folders 01 to 08; no personal data
- [ ] Docs, ADRs and the roadmap's Gap Analysis are updated where the change affects them
