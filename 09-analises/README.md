# Analyses

Each file answers one question with evidence: `NNNN-slug.md`, following the [`../docs/modelos/achado.md`](../docs/modelos/achado.md) template (or written through the `write-finding` skill). `montar.sh` does not touch this folder ([ADR 0005](../docs/adr/0005-evidencia-somente-leitura.md)).

Rules ([`AGENTS.md`](../AGENTS.md) §0 and §4):
- every claim cites a path and line, a patch, a hash or a run;
- every number says whether it was measured or estimated;
- temporary files stay outside the project.

Every analysis is written in English.

| # | Question | Confidence |
|---|---|---|
| [0001](0001-trafego-de-rede.md) | Which servers does the app connect to on its own? | measured |
| [0002](0002-entrada-virtual-wayland.md) | Can keystrokes be injected into rustode and the App on the virtual display? | measured |
