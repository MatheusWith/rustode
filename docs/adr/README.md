# Decisions (ADRs)

There is one decision per file, `NNNN-slug.md`, and its title states the decision itself. Template: [`../modelos/adr.md`](../modelos/adr.md). An accepted decision is never rewritten. Changes come in one of two ways:
- as a `## Addendum (YYYY-MM-DD): …` at the end;
- as a new ADR that supersedes the old one.

| # | Decision | Status |
|---|---|---|
| [0001](0001-bundle-sem-source-maps.md) | The production bundle is built without source maps | accepted (2026-11-16) |
| [0002](0002-dados-separados-do-code-oss.md) | The app's data lives in `.vscodium` and `.vscodium-shared`, separate from the system's Code - OSS | accepted (2026-11-16) |
| [0003](0003-sem-checagem-de-atualizacao.md) | The local build does not check for updates | accepted (2026-11-16) |
| [0004](0004-arvore-fonte-e-a-referencia.md) | The copied source tree is the reference for the analysis; the minified JavaScript only confirms | proposed |
| [0005](0005-evidencia-somente-leitura.md) | Folders 01 to 08 are read-only evidence; analyses go in `09-analises/` | proposed |
| [0006](0006-testes-em-tela-virtual.md) | UI tests run on a virtual display | proposed (in use) |

Possible statuses: `proposed`, `accepted`, `superseded by ADR NNNN`. The date and who decided go in parentheses.
