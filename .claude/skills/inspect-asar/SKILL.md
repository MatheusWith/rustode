---
name: inspect-asar
description: Examines the runtime modules packed in node_modules.asar (version, origin, whether it is native, whether it lives outside the asar) and checks them against the source tree. Use when you need to know which version of a dependency the app loads, where it sits in the package, or why a module lives outside the asar.
---

# Examine `node_modules.asar`

Material (relative to the project root):

- `02-engenharia-reversa/asar/node_modules.asar.lista.txt`: a list of everything in the asar
- `02-engenharia-reversa/asar/node_modules.asar.extraido/`: the extracted contents (including the files that live in `.unpacked`)
- `01-app/VSCode-linux-x64/resources/app/node_modules.asar.unpacked/`: native modules (`.node`) and whatever must stay outside the asar
- `01-app/VSCode-linux-x64/resources/app/node_modules/`: modules kept outside the asar on purpose (`semver`, `shell-quote`, `zod`, `@pondwader/socks5-server`, `@vscode/sandbox-runtime`)
- `02-engenharia-reversa/fonte/arvore/package.json` and `pnpm-lock.yaml`: what the source declares
- `02-engenharia-reversa/binarios.txt`: the type of each binary

## Process

### 1. Find the module

```sh
grep -E '^/(@[^/]+/)?<name>(/|$)' 02-engenharia-reversa/asar/node_modules.asar.lista.txt | head
```

If it isn't in the asar, look in the package's `node_modules/` and `node_modules.asar.unpacked/`.

### 2. Version and origin

```sh
jq -r '.name + " " + .version + " " + (.license // "?")' 02-engenharia-reversa/asar/node_modules.asar.extraido/<name>/package.json
grep -nE '"<name>"' 02-engenharia-reversa/fonte/arvore/package.json
grep -nE "^  '?/?<name>@" 02-engenharia-reversa/fonte/arvore/pnpm-lock.yaml | head
```

Say whether it is a direct dependency (it is in the root `package.json`) or a transitive one (only in the lockfile). VSCodium forks have the `@vscodium/` scope.

### 3. Native or outside the asar?

- The module's `.node`: `grep '<name>' 02-engenharia-reversa/binarios.txt`.
- Why it lives outside the asar: the exception list is in `02-engenharia-reversa/fonte/arvore/build/gulpfile.vscode.ts` (the `createAsar` call).

### 4. Who uses it

Look for the import in the source: `grep -rn --include='*.ts' -E "from '<name>'|import\\('<name>'\\)" 02-engenharia-reversa/fonte/arvore/src | head`. Remember that the agent host looks for modules in `node_modules.asar.unpacked` only when the app has a recorded commit ([`docs/aprendizado/0001`](../../../docs/aprendizado/0001-commit-muda-caminho-dos-modulos.md)).

### 5. Report

```text
Module:      <name> <version> (<license>)
In package:  asar | asar.unpacked (native) | node_modules (outside on purpose)
Declared:    direct in package.json | transitive (pnpm-lock.yaml)
Used by:     src/<file>.ts
```

## Rules

- Don't extract again inside the project. If you need your own extraction, use a temporary folder: `$NODE ~/Projects/vscode-vscodium/node_modules/asar/bin/asar.js extract <asar> "$(mktemp -d)"` ([`AGENTS.md`](../../../AGENTS.md) §0.2).
- To compare with another build, use the `compare-builds` skill.
