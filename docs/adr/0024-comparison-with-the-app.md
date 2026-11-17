# ADR 0024: rustode is compared with the App once per milestone, on the same virtual display, from a versioned list of scenarios

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session); no comparison has run yet (the first is planned for M3)
- **Build:** rustode at `d6debfb`; the App: local VSCodium 1.147.0

## Context

"Faster and leaner than VS Code" is a claim, and it needs numbers that can be compared. The study already has the pieces:

- an App that can be verified against the manifest (AGENTS.md §0.6);
- a virtual display ([ADR 0006](0006-testes-em-tela-virtual.md));
- a QA harness with a startup benchmark (`05-qa/suite/scenarios/startup_bench.cjs`) and memory guards (`05-qa/suite/scenarios/run_scenario.sh:26-35`).

ADR 0006 also records that absolute times on the virtual display do not hold for the real session. Relative comparisons on the same display do.

## Decision

- **Baseline:** the App, and only the App. The Reference (`code` 1.138) is not a baseline for the editor. The App's integrity is checked against the manifest before every comparison run.
- **Scenarios:** a versioned list that covers only operations both editors have:
  - opening 1 MB, 100 MB and 1 GB files;
  - 10k random inserts, then undoing all of them;
  - a regex search over a monorepo;
  - startup to first frame, cold and warm;
  - idle memory;
  - typing latency ([ADR 0025](0025-typing-latency-injected-input.md)).

  Scenarios that need the GUI join from M3.
- **Environment:** both editors run on the same `kwin_wayland --virtual` display, with software rendering for both.
- **Resources:** one editor instance at a time, under the machine's memory limit, with the QA abort threshold.
- **Cadence:** one report per milestone, with an updated Gap Analysis. The comparison with the App is never a per-pull-request gate ([ADR 0026](0026-micro-benchmark-gate.md) covers regressions between milestones).

Containment rules:

1. The App never runs in place from `01-app/`. It runs as a temporary copy on disk, checked against the manifest and for extra files before each run (AGENTS.md §11.5). Running it in place wrote three compile-cache files into `01-app/` on 2026-11-17 ([`09-analises/0002`](../../09-analises/0002-entrada-virtual-wayland.md), "Limitations").
2. The App gets an isolated profile (AGENTS.md §3.1) and no network unless the scenario needs it (§3.3). It is driven over CDP (§3.2), except for typing input ([ADR 0025](0025-typing-latency-injected-input.md)).
3. The memory guard follows the QA harness:
   - a run does not start below the scenario's need (the QA launcher refuses below 1,800 MB available, `05-qa/suite/scenarios/run_scenario.sh:26-28`);
   - a run aborts if available memory drops below 700 MB (`run_scenario.sh:34-35`; the watchdog in `05-qa/suite/collectors/sampler.py:183-184`).
4. Every result records the command, the folder it ran from, the run, the commit, the build profile and its measured/estimated label (AGENTS.md §0.4, §11.6). A result reports its repetitions and percentiles, never a single run.
5. Each milestone's report is a finding in `09-analises/`, and the Gap Analysis in `ROADMAP.md` is updated from it.

## The guarantee

Every comparison is like for like (same scenario, same display, same rendering, a verified App), and every number can be reproduced from its command.

## Consequences

- Measured on 2026-11-17: the App is intact. From `01-app/VSCode-linux-x64`, `sha256sum -c --quiet` against the manifest printed nothing, and the file list equals the manifest's 3,008 entries.
- A relocated copy may not reuse the App's Node compile cache. If it does not, cold-start numbers differ from those of in-place runs ([`lacunas.md`](../lacunas.md) G14).
- The App side of "first frame" and of a typed key's frame still needs a defined event ([`lacunas.md`](../lacunas.md) G15). The QA startup benchmark measures "workbench ready" instead.
- The versioned scenario list does not exist yet. It comes with the benchmark runner (`rustode bench`, M1).
