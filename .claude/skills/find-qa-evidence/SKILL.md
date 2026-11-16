---
name: find-qa-evidence
description: Finds the test run, steps, measurements and screenshots that support (or refute) a claim about the app, discarding invalid runs. Use when an analysis needs test evidence, or to find out whether a behavior has already been tested.
---

# Find QA evidence

Material (relative to the project root):

- `08-testes-do-build/RESUMO.md`: every run on the build, with each one's result
- `08-testes-do-build/<run>/`, which holds:
  - `steps.json`: one record per step (`id`, `status`, `observed`, `latency_ms`);
  - `screenshots/<id>.png`;
  - `sampler.jsonl`: memory every 1 s;
  - depending on the test, `network-report.md`, `capacity-*.json` or `resumo.json` (startup).
- `05-qa/results/`: every run, including those of the 2026-11-13 QA (reference and dev build)
- `05-qa/docs/`: the QA documents (feature inventory, findings P1–P12, build tests)
- `docs/qa.md`: summary and criteria

## Process

### 1. Find the run

```sh
grep -n '<word>' 08-testes-do-build/RESUMO.md
grep -rl --include=steps.json '<word>' 08-testes-do-build 05-qa/results
```

**Discard** runs with `INVALIDA`, `INTERROMPIDA`, `seletor-errado` or `runN` in the name ([`AGENTS.md`](../../../AGENTS.md) §0.5).

### 2. Read the step

```sh
jq -r '.[] | select(.id=="<ID>") | "\(.status) | \(.observed)"' <run>/steps.json
```

The step's screenshot is `<run>/screenshots/<ID>.png`. For memory over the test, use `sampler.jsonl`. Its `total_rss_kb` sums processes and counts shared memory twice, so prefer the PSS in the `capacity-*.json` files.

### 3. Compare with the reference

The same suite ran on `code` 1.138 (`…-code1.138…` on 2026-11-16, or `2026-11-13-code1.138.0-…`). Times are only comparable between runs on the same display. The 2026-11-16 runs used a virtual display without a GPU.

### 4. Report

```text
Run:         08-testes-do-build/<run>/ (valid)
Step:        <ID> <status>: <observed>
Screenshot:  <run>/screenshots/<ID>.png
Reference:   <1.138 run> <status>
Confidence:  measured
```

If no run covers the claim, say so and record the gap in `docs/lacunas.md`. Don't extend a result to a case that wasn't tested.
