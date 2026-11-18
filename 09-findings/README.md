# Analyses

Each file answers one question with evidence: `NNNN-slug.md`, following the [`../docs/templates/finding.md`](../docs/templates/finding.md) template (or written through the `write-finding` skill). `assemble.sh` does not touch this folder ([ADR 0005](../docs/adr/0005-evidence-is-read-only.md)).

Rules ([`AGENTS.md`](../AGENTS.md) §0 and §4):
- every claim cites a path and line, a patch, a hash or a run;
- every number says whether it was measured or estimated;
- temporary files stay outside the project.

Every analysis is written in English.

| # | Question | Confidence |
|---|---|---|
| [0001](0001-network-traffic.md) | Which servers does the app connect to on its own? | measured |
| [0002](0002-virtual-wayland-input.md) | Can keystrokes be injected into rustode and the App on the virtual display? | measured |
