---
name: compare-builds
description: Checks the app's integrity against the SHA-256 manifest and compares two builds (new, removed and changed files, by area, with a diff of the formatted JavaScript and of the asar listing). Use when the app is rebuilt, when there is another build to compare, or before concluding anything about a binary.
---

# Compare builds

The manifest `02-engenharia-reversa/manifesto-sha256.txt` is in `sha256sum` format, with paths relative to `01-app/VSCode-linux-x64/`.

## Process

### 1. Integrity (always first)

```sh
cd 01-app/VSCode-linux-x64 && sha256sum -c --quiet ../../02-engenharia-reversa/manifesto-sha256.txt; echo "rc=$?"
```

`rc=0` with no output means intact. Any `FAILED` line invalidates conclusions about that file ([`AGENTS.md`](../../../AGENTS.md) §0.6).

### 2. The other build's manifest

Generate the other app's manifest, with relative paths. Write it to a temporary folder or into the other app itself:

```sh
(cd <other-app> && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$TMP/other.sha256"
```

### 3. Difference by path

```bash
# separator = tab: some paths contain spaces (e.g. "Regular Expressions (JavaScript).tmLanguage")
tsv() { sed -E 's/^([0-9a-f]{64}) [ *](.*)$/\2\t\1/' "$1" | LC_ALL=C sort -t $'\t' -k1,1; }
LC_ALL=C join -t $'\t' -a1 -a2 -e MISSING -o 0,1.2,2.2 \
  <(tsv 02-engenharia-reversa/manifesto-sha256.txt) <(tsv "$TMP/other.sha256") \
  | awk -F'\t' '$2=="MISSING"{print "new\t" $1; next} $3=="MISSING"{print "removed\t" $1; next} $2!=$3{print "changed\t" $1}'
```

Tested on 2026-11-16:
- the manifest against itself prints nothing;
- a copy with one hash swapped and one line fewer prints exactly `changed` and `removed`.

Group the result by area:
- `resources/app/out/`;
- `node_modules.asar`;
- `node_modules.asar.unpacked/` (natives);
- `extensions/`;
- `node-compile-cache/`;
- the root binaries.

### 4. Detail by type

- **Changed JS:** format both with esbuild (`AGENTS.md` §6, "Tools") and use `diff -u`. Minified code changes names between builds, so look for differences in anchors, not in names.
- **Changed `node_modules.asar`:** compare the listings (`asar list`), then the `package.json` of the modules that changed.
- **Changed binary:** compare the `file` output (`02-engenharia-reversa/binarios.txt`) and the size.
- **`product.json`:** `diff <(jq -S . a) <(jq -S . b)`. `commit`, `date` and `checksums` always change between builds.
- **`node-compile-cache/`** always changes between builds and depends on the UID; it is not a sign of a code difference.

### 5. Report

```text
Integrity:   ok | N files failed
Difference:  N new, N removed, N changed (by area)
Relevant:    <files whose change alters behavior, with the anchor that changed>
Expected:    commit/date/checksums, node-compile-cache
```
