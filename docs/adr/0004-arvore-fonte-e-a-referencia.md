# ADR 0004: The copied source tree is the reference for the analysis; the minified JavaScript only confirms

- **Status:** proposed (2026-11-16, created with the project documents; awaiting the owner)
- **Build:** local VSCodium 1.147.0

## Context

The app has no source maps in its main bundles ([ADR 0001](0001-bundle-sem-source-maps.md)), and the JavaScript names stay minified even after reformatting (`out-formatado/`). On the other hand, two facts make the source usable:
- `02-engenharia-reversa/fonte/arvore/` is exactly the code that produced the app (commit + patches + post-install steps).
- 30 of the 32 bundles start from a file with the same path in `src/` ([`fonte-vs-pacote.md`](../fonte-vs-pacote.md)).

## Decision

Explanations of behavior cite the source tree (file and line). The packaged JavaScript serves to confirm two things: that the snippet is in the package, and what it became after the build. It is never the only source of an explanation when the source tree has the snippet.

Containment rules:

1. To get from the minified code to the source, use stable anchors, not variable names. Stable anchors are strings, command IDs, configuration and NLS keys, and property names.
2. A claim based only on the minified code is marked as such, with the reason. One example is code generated during the build with no file in the tree, like `nls.messages.js`.
3. Before using the package as evidence, confirm its integrity against the manifest ([`AGENTS.md`](../../AGENTS.md) §0.6).

## The guarantee

Every explanation points to readable code that anyone can open, and the package is only taken as evidence after it has been checked.

## Consequences

- Analyses become traceable and reviewable.
- Some pieces are generated during the build: NLS, the final `product.json`, the compile cache. They can only be explained from the package and the build scripts, and each analysis says so explicitly.
