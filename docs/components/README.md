# Components

One document per studied subsystem, `<area>.md`, following the [`../templates/component.md`](../templates/component.md) template. The list of processes and bundles, for choosing where to start, is in [`../architecture.md`](../architecture.md).

No components are documented yet. Candidates, in suggested order (gap G1):

| Component | Bundle | Why |
|---|---|---|
| Main process | `main.js`, `mainImpl.js` | entry point; lifecycle, windows, update turned off (ADR 0003) |
| Agent host | `vs/platform/agentHost/node/agentHostMain.js` | the component the port changed the most (patches 53 and 99) |
| Extension host | `vs/workbench/api/node/extensionHostProcess.js` | the extension API and the allowed proposals (G6) |
| Extension gallery | inside `workbench.desktop.main.js` and the shared process | Open VSX, the malicious list, patches 60 and 61 |
| Workbench layout | inside `workbench.desktop.main.js` | empty secondary sidebar (G7) |
