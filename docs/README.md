# rustode documentation

This is the index of the reference documents. The project rules are in [`AGENTS.md`](../AGENTS.md), and the current state is in [`ROADMAP.md`](../ROADMAP.md).

| Document | What for |
|---|---|
| [`arquitetura.md`](arquitetura.md) | processes, bundles, layers and data directories of the analyzed app |
| [`fonte-vs-pacote.md`](fonte-vs-pacote.md) | where each part of the package comes from: bundle → entry file, extensions, `product.json`, asar |
| [`build.md`](build.md) | how the app was built: script order, variables, pitfalls and how to reproduce |
| [`patches.md`](patches.md) | every patch: area and status (unchanged, adapted, port-only, skipped) |
| [`qa.md`](qa.md) | build test results, criteria and where each piece of evidence is |
| [`lacunas.md`](lacunas.md) | open questions and known limitations (G1…), in priority order |
| [`glossario.md`](glossario.md) | terms used in the project |
| [`padrao-de-documentos.md`](padrao-de-documentos.md) | filmcraft's documents, their roles, and how the standard was brought here |
| [`adr/`](adr/) | decisions (ADR 0001…), one per file |
| [`componentes/`](componentes/) | one document per studied subsystem |
| [`aprendizado/`](aprendizado/) | lessons that serve other projects |
| [`modelos/`](modelos/) | templates for ADRs, findings, components, patches, gaps and provenance |

Conventions (the same as filmcraft, where the standard came from):

- **Language.** Every document is written in English. Quotes from evidence stay verbatim.
- **File names.** Lowercase with hyphens. ADRs, analyses and lessons are numbered with 4 digits (`0001-slug.md`). Existing names keep their Portuguese spelling, because scripts and links depend on them.
- **Tracking documents** (architecture, source-vs-package, qa, gaps) open with a metadata line and close with `## Revision history`. The metadata line reads `> **Last reviewed:** … · **Last updated:** … · **Change:** trivial|minor|major (…) · **Target:** …`.
- **Diagrams** are text blocks (```` ```text ````). There is no front matter.
- **Numbers** say whether they were measured or estimated, and where they came from ([`AGENTS.md`](../AGENTS.md) §0.4).
