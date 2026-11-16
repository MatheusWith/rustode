---
name: write-finding
description: Records a finished analysis in 09-analises/ in the project format (question, short answer, evidence, limitations), in English, with numbering and the index.
disable-model-invocation: true
---

# Record a finding

Turns what was discovered in the conversation into `09-analises/NNNN-slug.md`, written in English. Don't interview the owner: use what is already in the conversation and the evidence.

## Process

### 1. Gather the evidence

Reread the conversation and list each claim with its evidence (path:line, patch, hash, run). A claim without evidence goes to "Limitations" as a hypothesis ([`AGENTS.md`](../../../AGENTS.md) §0.3). Check that every cited path exists (`test -e`).

### 2. Choose the number and name

```sh
ls 09-analises/ | grep -E '^[0-9]{4}-' | sort | tail -1
```

Use the next number and a short slug, lowercase with hyphens. Confirm the slug with the owner in one line before writing.

### 3. Write it

Use [`docs/modelos/achado.md`](../../../docs/modelos/achado.md), which has these sections:
- Question;
- Short answer;
- Evidence: a table, plus the commands with the folder they ran from;
- Limitations;
- Next steps.

Confidence is `measured` only if everything in the short answer was measured.

### 4. Update the indexes

- add a row to `09-analises/README.md`;
- if it resolved a gap, mark the gap in `docs/lacunas.md` and point it to the analysis;
- record it in `ROADMAP.md`: "Progress log", "Last updated" and "Recorded analyses".

## Rules

- Never write to `01-app/` to `08-testes-do-build/` (§0.1).
- No personal data (§2).
- One analysis, one question. If two came up, they are two files.
