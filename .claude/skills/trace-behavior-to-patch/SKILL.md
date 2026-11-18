---
name: trace-behavior-to-patch
description: Links app behavior that differs from the original VS Code to the patch (or product.json key) that causes it, and to the reason recorded in PORT-NOTES. Use when the app does something different from VS Code/Code - OSS, or to find out where a piece of the source tree came from.
---

# Link a behavior to a patch

Every difference from the original VS Code has a traceable cause: a patch, a JSON action or a `product.json` key ([`AGENTS.md`](../../../AGENTS.md) §4.2).

Sources (relative to the project root):

- `02-package-analysis/source/port-applied.diff`: everything that changed from the base commit to the build tree
- `06-port/patches/orig/` (as it came from VSCodium) and `06-port/patches/port/` (adapted to 1.147, or port-only, 96 to 99)
- `06-port/PORT-NOTES.md`: the reason for each adaptation and local decision
- `docs/patches.md`: a table of every patch and its status
- `02-package-analysis/product.json`: the packaged `product.json`

## Process

### 1. Describe the difference

One sentence: "in the app, X happens; in VS Code, Y happens". If the difference came from a test, cite the run (`08-build-tests/<run>/`).

### 2. Find the responsible code

Use the `find-minified-symbol` skill or search for anchors directly in the tree. Done when you have `src/<file>.ts:L`.

### 3. Check whether the file was changed

```sh
grep -n '^diff --git a/<relative-path>' 02-package-analysis/source/port-applied.diff
grep -ln '<relative-path>' 06-port/patches/orig/*.patch 06-port/patches/orig/linux/*.patch 06-port/patches/port/*.patch 06-port/patches/port/linux/*.patch
```

- **No patch touches the file:** look for the cause in two places.
  - `product.json`: defaults, `extensionsGallery`, `dataFolderName`, `extensionEnabledApiProposals`.
  - A configuration whose default is changed by another file.
- **A patch touches it:** read the hunk and confirm that it covers the line from step 2.

### 4. Find the reason

- Patch in `patches/port/`: look up its name in `06-port/PORT-NOTES.md` (the patch table or "Decisões locais").
- Patch only in `patches/orig/`: it is VSCodium's, unchanged; the reason is VSCodium's intent (`07-vscodium-source/docs/`).
- Patch 96 to 99 or a local decision: also see `docs/adr/`.

### 5. Report

```text
Difference:  <the sentence from step 1>
Code:        src/<file>.ts:L
Cause:       patch <name> (orig | adapted | port-only) | product.json <key>
Reason:      <short quote from PORT-NOTES or VSCodium>
Evidence:    <test run or command>
```

## Patch prefixes

| Prefix | Family |
|---|---|
| `00-` | general (branding, UI, telemetry, security, built-in extensions) |
| `1x-` | version and update |
| `2x-` | VSCodium forks |
| `3x-` | build dependencies |
| `5x-` | Copilot |
| `6x-` | security and gallery |
| `9x-` | port-only |
| `linux/` | Linux packaging |
