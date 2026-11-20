# Decisions (ADRs)

There is one decision per file, `NNNN-slug.md`, and its title states the decision itself. Template: [`../templates/adr.md`](../templates/adr.md). An accepted decision is never rewritten. Changes come in one of two ways:
- as a `## Addendum (YYYY-MM-DD): …` at the end;
- as a new ADR that supersedes the old one.

## Study (the App)

| # | Decision | Status |
|---|---|---|
| [0001](0001-bundle-without-source-maps.md) | The production bundle is built without source maps | accepted (2026-11-16) |
| [0002](0002-data-separate-from-code-oss.md) | The app's data lives in `.vscodium` and `.vscodium-shared`, separate from the system's Code - OSS | accepted (2026-11-16) |
| [0003](0003-no-update-check.md) | The local build does not check for updates | accepted (2026-11-16) |
| [0004](0004-source-tree-is-the-reference.md) | The copied source tree is the reference for the analysis; the minified JavaScript only confirms | proposed |
| [0005](0005-evidence-is-read-only.md) | Folders 01 to 08 are read-only evidence; analyses go in `09-findings/` | proposed |
| [0006](0006-tests-on-virtual-display.md) | UI tests run on a virtual display | proposed (in use); amendment proposed in [0028](0028-virtual-display-input.md) |

## Editor (rustode, Phase 1)

Decided by the owner in the Phase 1 grilling session (2026-11-16), except where the status says otherwise.

| # | Decision | Status |
|---|---|---|
| [0007](0007-own-git-repository.md) | rustode is its own git repository; the evidence folders and private notes stay out of it, and the first push waits for a privacy review | accepted (2026-11-16); first push pending (owner) |
| [0008](0008-rulebook-editor-section.md) | The agent rulebook gets an Editor section, and its evidence rules cover benchmark claims | accepted (2026-11-16) |
| [0009](0009-mit-and-reimplemented-behavior.md) | The editor code is MIT; behavior studied in VS Code is reimplemented, never translated, and credited | accepted (2026-11-16) |
| [0010](0010-ecosystem-crates-allowed.md) | Mature ecosystem crates are allowed; "from scratch" applies only to VS Code code | accepted (2026-11-16) |
| [0011](0011-english-and-pinned-toolchain.md) | Code, comments, commits and documents are in English; the editor uses Rust 2024 on a pinned stable toolchain | accepted (2026-11-16) |
| [0012](0012-linux-x64-only.md) | Phase 1 builds, tests and benchmarks on Linux x64 only | accepted (2026-11-16) |
| [0013](0013-crates-and-workspace-api.md) | One crate per module, and the workspace crate is the only rustode crate the frontends depend on | accepted (2026-11-16) |
| [0014](0014-in-process-frontends.md) | The frontends link the workspace in-process, send it commands and render its state; other transports come later through the protocol types | accepted (2026-11-16) |
| [0015](0015-child-process-servers.md) | Language servers and debug adapters run as child processes over stdio, supervised by the workspace | accepted (2026-11-16); not implemented (M5, M7) |
| [0016](0016-tui-ratatui.md) | The TUI is built on ratatui and has every Phase 1 feature except the minimap | accepted (2026-11-16); implemented in part (M2) |
| [0017](0017-gui-egui-wgpu.md) | The GUI is egui on eframe with wgpu; it repaints only on change and draws text with its own virtualized widget | accepted (2026-11-16); not implemented (M3) |
| [0018](0018-phase-1-milestones.md) | Phase 1 ships in ten milestones, M0 to M9, with a fixed scope and fixed exclusions | accepted (2026-11-16) |
| [0019](0019-tests-at-three-seams.md) | Tests are written first, at three seams: the workspace API, input to frame, and the CLI process | accepted (2026-11-16) |
| [0020](0020-coverage-gates.md) | Line coverage is at least 85% on the core crates and 60% on the frontends, and the thresholds are never lowered | accepted (2026-11-16) |
| [0021](0021-strict-lints.md) | Clippy runs pedantic with warnings denied and complexity limits; every exception is justified | accepted (2026-11-16) |
| [0022](0022-commits-branches-pull-requests.md) | Commits follow Conventional Commits with closed types and scopes, branches are typed, and main changes only through squash-merged pull requests | accepted (2026-11-16); branch protection not verified |
| [0023](0023-same-checks-local-and-ci.md) | The same checks run in git hooks and in CI, and the security scans run offline | accepted (2026-11-16); registry rules in CI not applied |
| [0024](0024-comparison-with-the-app.md) | rustode is compared with the App once per milestone, on the same virtual display, from a versioned list of scenarios | accepted (2026-11-16); first run planned for M3 |
| [0025](0025-typing-latency-injected-input.md) | Typing latency is measured with keystrokes injected into both editors through the virtual compositor | accepted (2026-11-16, owner's option a); waits for 0028 |
| [0026](0026-micro-benchmark-gate.md) | Micro-benchmarks block regressions above 5% locally and 10% in CI | accepted (2026-11-16); applied |
| [0027](0027-ropey-or-piece-tree.md) | Keep ropey for the text buffer; replace it with a custom piece tree only if the benchmarks show it behind VS Code's text model | proposed (left open by the owner) |
| [0028](0028-virtual-display-input.md) | Keystrokes may be injected on the virtual display through the virtual compositor's own input paths (amends ADR 0006 rule 2 and AGENTS.md §3.2) | proposed — owner's decision (2026-11-17, from finding 0002) |
| [0029](0029-extension-management-open-vsx.md) | Extensions are managed through Open VSX's REST API, stored in VS Code's layout in rustode's own folder, and never loaded | proposed (2026-11-17, ticket 40); the folder and the SHA-256 refusal are owner decisions |
| [0030](0030-dap-client.md) | The debug adapter client starts with a tracer (launch, breakpoints, stop, continue) over the LSP child-process supervision; extended with stepping, the call stack, variables, watch and the console | accepted (2026-11-19, tickets 37 and 38); the owner confirmed `rustode.debugAdapters` as freely modifiable and live |
| [0031](0031-editor-groups.md) | Editor groups are a tree of splits in the workspace; one document and one undo history, and a cursor set for each view that follows every edit | accepted (2026-11-19, ticket 35) |

Possible statuses: `proposed`, `accepted`, `superseded by ADR NNNN`. The date and who decided go in parentheses. "Proposed — owner's decision" marks a proposal that only the owner can accept, because it changes an inviolable rule.
