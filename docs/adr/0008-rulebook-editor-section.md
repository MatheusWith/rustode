# ADR 0008: The agent rulebook gets an Editor section, and its evidence rules cover benchmark claims

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in [`AGENTS.md`](../../AGENTS.md) §11 and in ADRs 0007 to 0028)
- **Build:** rustode at `d6debfb`

## Context

`AGENTS.md` is the single source of instructions for agents and contributors. `CLAUDE.md` only imports it ([`padrao-de-documentos.md`](../padrao-de-documentos.md) §2). Until Phase 1 it covered only the study:

- the inviolable rules on evidence, licenses, personal data, running the App and analysis (§0–§4);
- the working instructions (§5–§10).

Editor work adds crates, tests, hooks, commits and benchmarks. Its headline numbers (startup, memory, typing latency) are exactly the kind of claim that §0.3 and §0.4 exist to discipline. The Phase 1 decisions themselves were made in a planning conversation and recorded only in the owner's local notes.

## Decision

`AGENTS.md` has two parts: the Study rules (§0–§10) and the Editor rules (§11). §0–§4 bind both. Every decision from the Phase 1 grilling session is recorded as an ADR, numbered from 0007.

Containment rules:

1. §0–§4 are not reworded for the editor. They only gain cross-reference sentences that point to §11 (in §0.4 and §3.4).
2. The editor rules live in §11. The commit, branch and check conventions that the hooks enforce stay in [`CONTRIBUTING.md`](../../CONTRIBUTING.md), "Editor code: commits, branches and checks", because every hook error points there (`xtask/src/conventions.rs:32`).
3. Benchmark and performance claims follow §0.3 and §0.4: they cite their evidence and say whether each number was measured (with its command) or estimated (with its source). §11.6 says how.
4. A change to an inviolable rule goes through its own ADR and the owner's decision. The native-input amendment needed for typing latency is [ADR 0028](0028-virtual-display-input.md), left as the owner's decision.
5. One decision stays open: replacing ropey with a custom piece tree ([ADR 0027](0027-ropey-or-piece-tree.md), proposed).

## The guarantee

One file still holds every instruction, and nothing in the Editor section weakens §0–§4.

Check: `git diff d6debfb -- AGENTS.md` shows §0–§4 changed only by added cross-reference sentences.

## Consequences

- An agent starting editor work reads §5 and then §11, the way an agent starting an analysis reads §5 and then §4.
- 22 ADRs (0007–0028) record the Phase 1 decisions. The index lists them apart from the study's ADRs 0001–0006 ([`README.md`](README.md)).
- A later Phase 1 decision is a new ADR, or an addendum to an existing one, never an edit of an accepted ADR ([`README.md`](README.md)).
