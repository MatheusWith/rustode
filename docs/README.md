# rustode documentation

This is the index of the reference documents. The project rules are in [`AGENTS.md`](../AGENTS.md), and the current state is in [`ROADMAP.md`](../ROADMAP.md).

| Document | What for |
|---|---|
| [`architecture.md`](architecture.md) | processes, bundles, layers and data directories of the analyzed app |
| [`source-vs-package.md`](source-vs-package.md) | where each part of the package comes from: bundle → entry file, extensions, `product.json`, asar |
| [`build.md`](build.md) | how the analyzed App (VSCodium) was built: script order, variables, pitfalls and how to reproduce. It is not the editor's build: for that, see the README's "Get started" and CONTRIBUTING's "Setup" |
| [`patches.md`](patches.md) | every patch: area and status (unchanged, adapted, port-only, skipped) |
| [`qa.md`](qa.md) | build test results, criteria and where each piece of evidence is |
| [`gaps.md`](gaps.md) | open questions and known limitations (G1…), in priority order |
| [`glossary.md`](glossary.md) | terms used in the project |
| [`document-standard.md`](document-standard.md) | filmcraft's documents, their roles, and how the standard was brought here |
| [`adr/`](adr/) | decisions (ADR 0001…), one per file |
| [`components/`](components/) | one document per studied subsystem |
| [`learnings/`](learnings/) | lessons that serve other projects |
| [`templates/`](templates/) | templates for ADRs, findings, components, patches, gaps and provenance |

Conventions (the same as filmcraft, where the standard came from):

- **Language.** Every document is written in English. Quotes from evidence stay verbatim.
- **File names.** Lowercase with hyphens. ADRs, analyses and lessons are numbered with 4 digits (`0001-slug.md`). Names are in English; only names copied verbatim from the live sources stay in Portuguese (AGENTS.md, "Language").
- **Tracking documents** (architecture, source-vs-package, qa, gaps) open with a metadata line and close with `## Revision history`. The metadata line reads `> **Last reviewed:** … · **Last updated:** … · **Change:** trivial|minor|major (…) · **Target:** …`.
- **Diagrams** are text blocks (```` ```text ````). There is no front matter.
- **Numbers** say whether they were measured or estimated, and where they came from ([`AGENTS.md`](../AGENTS.md) §0.4).
