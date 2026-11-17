# Document standard (from filmcraft)

> **Last reviewed:** 2026-11-17 · **Last updated:** 2026-11-17 · **Change:** minor (the workflow skills row) · **Target:** the `~/Projects/filmcraft` standard applied to rustode

rustode follows the documentation standard of the filmcraft project (`~/Projects/filmcraft`). The survey was done on 2026-11-16 by three read-only exploration agents: one for the root documents, one for `docs/` and one for the agent configuration. This document lists:
- what filmcraft has;
- the role of each piece;
- how it was brought here.

## 1. filmcraft's documents and their roles

| Document | Role | Organization |
|---|---|---|
| `AGENTS.md` | inviolable rules for humans and agents; takes precedence over `CLAUDE.md` | numbered sections from `## 0.`; citable numbered rules (§0.3), each with a bold opening sentence, the check that enforces it and the single exception |
| `CLAUDE.md` | session start sequence for agents | identity paragraph; "Start every session here" (steps with a public alternative); "Non-negotiables" (bullets citing AGENTS §§) |
| `README.md` | showcase, installation, document index, honest status, license | HTML header with badges, `> [!NOTE]`, anchor navigation, sections with screenshots, index table (`| | |`), "Status" with stage and gaps |
| `ROADMAP.md` | public progress dashboard | stage banner; metadata line (`Last reviewed · Last updated · Change · Target`); tables of numbers marked measured or estimated; prioritized "Upcoming" with hours; dated progress log; revision history (`trivial/minor/major`) |
| `CONTRIBUTING.md` | an entry point that only forwards | 3-step list; "More reading"; inbound license |
| `ATTRIBUTION.md` | machine-checked index of all third-party material | sections by category, each with a preamble and an `Asset · Author · Source · Licence` table; `.attribution` sidecar files next to each asset |
| `NOTICE` | required notices (Apache style) | copyright, dual license, trademarks, one line per third-party material |
| `LICENSE-MIT`, `LICENSE-APACHE` | dual license | standard texts |
| `docs/*.md` (23) | architecture, testing, contributing, protocol, parity, gaps, performance | lowercase with hyphens; tracking documents with a metadata line and revision history; text diagrams |
| `docs/adr/0001-….md` | decisions | `# ADR NNNN: <decision>`; status as a list; Context · Decision (containment rules) · Guarantee · Consequences · dated addenda |
| `docs/learning/` | study guide in pt-BR (not versioned) | numbered chapters; "Sources:"; "Transferable concept"; "Taking it to your projects" |
| crate READMEs | each component's contract | identity, provenance, specs, API, tests (file → property), performance with method, gaps always last |
| `.opencode/skills/*/SKILL.md` + `agents/openai.yaml` | skills for agents | front matter `name`, `description` ("Use when…"), `disable-model-invocation`; body with `## Process` and numbered steps with a done criterion |
| `.serena/project.yml`, `.ai-memory.toml`, `.mcp.json` | tool configuration | language server, memory scope, the app's MCP servers |
| `.github/workflows/*.yml` | CI | an opening comment saying what it checks and what it leaves out; minimal permissions |

Precedence in filmcraft: `AGENTS.md` > `CLAUDE.md` > `docs/*`; `CONTRIBUTING.md` only forwards.

## 2. How it was brought to rustode

| filmcraft | rustode | Difference and reason |
|---|---|---|
| `AGENTS.md` (rules) + `CLAUDE.md` (work sequence) | [`AGENTS.md`](../AGENTS.md) with everything; [`CLAUDE.md`](../CLAUDE.md) = `@AGENTS.md` | single source, requested by the owner: OpenCode and Codex read only `AGENTS.md`, and two files tend to diverge. The rules (§0–§4) still take precedence over the instructions (§5–§9, with the skills in §7) |
| the "never hang", "clean-room" and "no Adobe assets" rules | §0 untouched evidence, §1 licenses and scope, §2 personal data, §3 running the app, §4 analysis rules | the risk here is tampering with evidence, not violating a license: the material is MIT |
| `README.md` with badges and screenshots | [`README.md`](../README.md) without badges | a local project; the notice, navigation, honest status, index and license were kept |
| `ROADMAP.md` | [`ROADMAP.md`](../ROADMAP.md) | same shape; "Owner decision" marks what depends on the owner |
| `CONTRIBUTING.md`, `ATTRIBUTION.md`, `NOTICE` | [`CONTRIBUTING.md`](../CONTRIBUTING.md), [`ATTRIBUTION.md`](../ATTRIBUTION.md), [`NOTICE`](../NOTICE) | the categories became analyzed software, downloaded extensions, patches, screenshots, tools and runtime downloads |
| (no docs index) | [`docs/README.md`](README.md) | added: filmcraft doesn't have one |
| `docs/architecture.md`, `gaps.md`, `testing.md`, parity docs | [`arquitetura.md`](arquitetura.md), [`lacunas.md`](lacunas.md), [`qa.md`](qa.md), [`fonte-vs-pacote.md`](fonte-vs-pacote.md) | "parity" here is source × package |
| `docs/adr/` | [`docs/adr/`](adr/) with an index | 6 ADRs for the build decisions (3 accepted, 3 proposed) |
| `docs/learning/` | [`docs/aprendizado/`](aprendizado/) | a short, numbered lesson format |
| crate READMEs | [`docs/componentes/`](componentes/) + [`docs/modelos/componente.md`](modelos/componente.md) | one document per studied subsystem |
| `.attribution` (sidecar per asset) | [`docs/modelos/proveniencia.md`](modelos/proveniencia.md) | a `.procedencia` sidecar for extracted or captured material |
| (no analyses) | [`09-analises/`](../09-analises/) | numbered findings; out of `montar.sh`'s reach |
| `.opencode/skills/` (generic) | [`.claude/skills/`](../.claude/skills/) (original) and [`.opencode/skills/`](../.opencode/skills/) (copy) | 7 reverse-engineering skills of its own + `ast-grep` and `grilling` copied from filmcraft + REA's `reverse-engineer-anything`. The original lives in `.claude/skills` (Claude Code), and `sincronizar-skills.sh` keeps the identical copy for OpenCode, without links |
| `.serena/project.yml`, `.ai-memory.toml` | the same, adapted | Serena with TypeScript on the source tree, read-only; ai-memory scoped to `rustode` |
| `.codegraph/` | created by the owner | indexing is the owner's decision (ROADMAP, next step 6) |
| `.mcp.json` (the app's servers) | [`.mcp.json`](../.mcp.json) and [`opencode.json`](../opencode.json) with only REA | filmcraft's servers don't apply; Serena, CodeGraph and ai-memory are already global |
| the `implement`, `tdd`, `code-review`, `to-tickets`, `grill-with-docs` and `wait-what` skills | not copied at first. With the editor, `implement`, `tdd`, `to-tickets` and `grill-with-docs` are in `.claude/skills/`, together with `grill-me` and `to-spec` (first commit `78aa4d5`; OpenCode copy `06e4dca`). `code-review` and `wait-what` are still not copied | the editor brought a git repository and code under development. Some of these skills still point to skills and files that this project does not have ([`AGENTS.md`](../AGENTS.md) §7) |

**Language.** filmcraft writes its documents in English (except the pt-BR study guide). rustode first wrote them in Portuguese and, at the owner's request on 2026-11-16, switched to English. Folder and file names kept their Portuguese spelling, because `montar.sh` and the links depend on them.

## 3. filmcraft problems that were not copied

- Mixed British and American spelling (licence/license).
- One `CLAUDE.md` bullet pasted into another, left over from a merge.
- A paragraph without a heading in `AGENTS.md` §3.2, copied from another repository.
- Loose bullets above the `ROADMAP.md` banner.
- Skills that point to skills and files that don't exist.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-17 | minor | The workflow skills that came with the editor (`implement`, `tdd`, `to-tickets`, `grill-with-docs`, `grill-me`, `to-spec`) |
| 2026-11-16 | minor | Translated to English; English is now the documents' language; REA and `.mcp.json` rows updated |
| 2026-11-16 | major | Created from the filmcraft survey |
