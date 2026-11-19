---
name: find-minified-symbol
description: Locates in the source tree the code that produced a minified snippet of the app (stack trace, out-formatted snippet, observed behavior), using stable anchors and the NLS indexes. Use when you need to explain a snippet of the packaged JavaScript or link an app error to its source .ts file.
---

# Locate a minified snippet in the source

The bundles have no source maps ([ADR 0001](../../../docs/adr/0001-bundle-without-source-maps.md)) and local names are minified. The way to the source goes through **anchors**: what survives minification unchanged. Anchors are strings, command IDs, configuration and context keys, property and method names, and NLS indexes. The final reference is always the source tree ([`AGENTS.md`](../../../AGENTS.md) §4.1).

Folders (relative to the project root):

- `01-app/VSCode-linux-x64/resources/app/out/`: the original bundles (stack-trace positions refer to these)
- `02-package-analysis/out-formatted/`: the same bundles reformatted (line numbers change)
- `02-package-analysis/source/tree/src/`: the source

## Process

### 1. Find the bundle and the snippet

- **Stack trace** (`…/out/<path>.js:L:C`):
  1. Read the original, which is one long line: `awk -v L=<L> 'NR==L' 01-app/VSCode-linux-x64/resources/app/out/<path>.js | cut -c$((C-300))-$((C+300))`.
  2. Find the same snippet in `out-formatted/` to read it with indentation.
- **Observed behavior:** start from a string that appears in the UI or the log and search for it in `out-formatted/`.

Done when you have a 5- to 30-line snippet of the formatted bundle.

### 2. Extract anchors

From the snippet, note:

- literal strings (`"…"`) and error messages;
- command IDs (`workbench.action.…`), configuration keys (`editor.…`, `chat.…`) and context keys;
- property and method names, which are not minified, and class names when they are preserved;
- **NLS indexes:** calls like `<f>(N, null, …)`. Translate the index into text, module and key:

  ```sh
  O=01-app/VSCode-linux-x64/resources/app/out; N=<index>
  node -e "const m=require('./$O/nls.messages.json'),k=require('./$O/nls.keys.json');let i=0;for(const [mod,keys] of k){for(const key of keys){if(i===+process.argv[1]){console.log(JSON.stringify({text:m[i],module:mod,key:key}));process.exit(0)}i++}}" $N
  ```

  The module (`vs/…`) is the file `src/<module>.ts`, and the key appears in its `localize('<key>', …)`.

Local variable names don't work as anchors.

### 3. Search the tree

```sh
grep -rn --include='*.ts' -F '<anchor>' 02-package-analysis/source/tree/src
```

With several anchors, keep the files that contain all of them (or most). To confirm the shape of the code (a class with method X that calls Y), use the `ast-grep` skill on the tree.

### 4. Confirm and state the confidence

- **high:** two or more independent anchors in the same file, or an NLS index pointing to the module;
- **medium:** one rare anchor (command ID, configuration key) and a compatible structure;
- **low:** only a common string or only structure. Say it is a hypothesis.

### 5. Report

```text
Bundle:      out/<path>.js (formatted: lines A–B)
Source:      src/<file>.ts:L (function/class)
Anchors:     <list>
Confidence:  high | medium | low (reason)
Patch:       <if the file was changed by a patch; see the trace-behavior-to-patch skill>
```

## Pitfalls

- **Code generated during the build has no file in the source.** Examples:
  - `nls.messages.js`;
  - the product configuration injected into the bundle (`BUILD_INSERT_PRODUCT_CONFIGURATION`);
  - the agent host's crypto module (its own build).
- **The tree already includes the patches.** To know whether the snippet came from a port patch or from the original VS Code, compare with `02-package-analysis/source/port-applied.diff`.
- **Different bundles repeat modules.** `workbench.desktop.main.js` and `sessions.desktop.main.js` share `base`, `platform` and `editor`. Say which bundle the snippet is from.
