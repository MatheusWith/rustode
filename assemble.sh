#!/usr/bin/env bash
# Assembles or refreshes this project from the live sources (everything is a real COPY: no symbolic or hard
# links; the sources are not changed).
# Idempotent: run it again after new tests to bring in recent results, screenshots and logs.
# Names that come from the live sources (QA run folders, QA documents, port logs) are copied verbatim.
# Usage: ./assemble.sh            everything
#        SKIP_FORMAT=1 ...      skips the formatted JavaScript copies (the slowest part)
set -euo pipefail
DEST=$(cd "$(dirname "$0")" && pwd)
APP=$HOME/Projects/VSCode-linux-x64
WT=$HOME/Projects/vscode-vscodium
QA=$HOME/Projects/vscode/qa
PORT=$HOME/Projects/vscodium-port
VSCODIUM=$HOME/Projects/vscodium
NODE=$HOME/.local/opt/node-v24.22.1-linux-x64/bin/node
ASAR=$WT/node_modules/asar/bin/asar.js
ESBUILD=$(ls -d "$WT"/build/node_modules/.pnpm/@esbuild+linux-x64@*/node_modules/@esbuild/linux-x64/bin/esbuild | head -1)
RE=$DEST/02-package-analysis
step() { echo ">> $*"; }

step "01 built app (the build's target)"
mkdir -p "$DEST/01-app"
rsync -a --copy-links --delete "$APP/" "$DEST/01-app/VSCode-linux-x64/"

step "05 QA (documents, suite, results, git history)"
rsync -a --copy-links --delete "$QA/" "$DEST/05-qa/"

step "06 port (scripts, original and adapted patches, build logs)"
rsync -a --copy-links --delete "$PORT/" "$DEST/06-port/"

step "07 source VSCodium (reference: patches, docs, scripts, product.json)"
mkdir -p "$DEST/07-vscodium-source"
rsync -a --copy-links --delete --include='/patches/***' --include='/docs/***' --include='/src/***' --include='/*.sh' \
  --include='/product.json' --include='/announcements-*.json' --include='/LICENSE' --include='/README.md' --exclude='*' \
  "$VSCODIUM/" "$DEST/07-vscodium-source/"
git -C "$VSCODIUM" log -1 --format='%H %cs %s' >"$DEST/07-vscodium-source/COMMIT.txt"

step "03 documents (independent copies)"
rm -rf "$DEST/03-documents"
mkdir -p "$DEST/03-documents"
cp -a "$DEST/05-qa/docs" "$DEST/03-documents/qa"
cp -a "$DEST/06-port/PORT-NOTES.md" "$DEST/03-documents/PORT-NOTES.md"
cp -a "$DEST/07-vscodium-source/docs" "$DEST/03-documents/vscodium-docs"
for f in "$DEST/05-qa/results"/*/network-report.md; do
  if [ -f "$f" ]; then cp -a "$f" "$DEST/03-documents/network-$(basename "$(dirname "$f")").md"; fi
done

step "04 screenshots (test captures, independent copies, same folder tree)"
rm -rf "$DEST/04-screenshots"
mkdir -p "$DEST/04-screenshots"
(cd "$DEST/05-qa/results" && find . -type f \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' -o -iname '*.webp' \) -print0 |
  xargs -0 -r cp -a --parents -t "$DEST/04-screenshots")

step "02 package analysis: metadata, manifest, binaries"
mkdir -p "$RE/asar" "$RE/source"
cp "$APP/resources/app/product.json" "$APP/resources/app/package.json" "$RE/"
(cd "$DEST/01-app/VSCode-linux-x64" && find . -type f -print0 | sort -z | xargs -0 sha256sum) >"$RE/manifest-sha256.txt"
(cd "$DEST/01-app/VSCode-linux-x64" && find . -type f \( -name '*.node' -o -name '*.so*' -o -perm -u+x \) -print0 | sort -z | xargs -0 file) >"$RE/binaries.txt"
"$APP/bin/codium" --version >"$RE/version.txt" 2>&1 || true

step "02 asar: list and extraction of node_modules.asar"
"$NODE" "$ASAR" list "$APP/resources/app/node_modules.asar" >"$RE/asar/node_modules.asar.list.txt"
rm -rf "$RE/asar/node_modules.asar.extracted"
"$NODE" "$ASAR" extract "$APP/resources/app/node_modules.asar" "$RE/asar/node_modules.asar.extracted"

step "02 exact source of the build (commit + patches + post-install steps), without node_modules or outputs"
TMPL=$(mktemp)
(cd "$WT" && git ls-files -co --exclude-standard -z) >"$TMPL"
# --ignore-missing-args: git still lists tracked files that the port's patches removed from the worktree
# --copy-links: the repository's only link (a terminal-suggest test fixture) becomes a real file
rsync -a --copy-links --delete --ignore-missing-args --from0 --files-from="$TMPL" "$WT/" "$RE/source/tree/"
rm -f "$TMPL"
IDX=$(mktemp -u)
TREE=$(cd "$WT" && GIT_INDEX_FILE=$IDX git read-tree HEAD && GIT_INDEX_FILE=$IDX git add -A 2>/dev/null && GIT_INDEX_FILE=$IDX git write-tree)
rm -f "$IDX"
# --irreversible-delete: removed files (e.g. extensions/copilot) show only as removed, without their contents
(cd "$WT" && git diff --binary --irreversible-delete HEAD "$TREE") >"$RE/source/port-applied.diff"
(cd "$WT" && git diff --stat=200 --irreversible-delete HEAD "$TREE" | tail -1) >"$RE/source/port-applied.stat.txt"
{
  echo "VS Code (base):   $(git -C "$WT" rev-parse HEAD)  ($(git -C "$WT" log -1 --format='%cs %s' | cut -c1-100))"
  echo "VSCodium (source): $(cat "$DEST/07-vscodium-source/COMMIT.txt" | cut -c1-120)"
  echo "App version:       $(head -1 "$RE/version.txt")"
  echo "Electron:          $(grep -oE 'electron-v[0-9.]+' "$WT/build/checksums/electron.txt" | head -1)"
  echo "Build tree:        $TREE (git, object in the $WT repository)"
  echo "Assembled on:      $(date '+%F %T')"
} >"$RE/source/origin.txt"

step "08 build test results (copies of the runs from 2026-11-16 on) and SUMMARY.md"
rm -rf "$DEST/08-build-tests"; mkdir -p "$DEST/08-build-tests"
python3 -I - "$QA/results" "$DEST/08-build-tests" <<'PY'
import collections, json, os, shutil, sys
src, dst = sys.argv[1:]
FIRST = '2026-11-16'
rows = []
for name in sorted(os.listdir(src)):
	path = os.path.join(src, name)
	if name[:10] < FIRST or not name[:4].isdigit():
		continue
	if os.path.isdir(path):
		shutil.copytree(path, os.path.join(dst, name), symlinks=False)
	else:
		shutil.copy2(path, dst)
		continue
	verdict = ''
	steps = os.path.join(path, 'steps.json')
	if os.path.exists(steps):
		c = collections.Counter(x['status'] for x in json.load(open(steps)))
		verdict = ', '.join(f'{v} {k}' for k, v in c.most_common())
	rep = os.path.join(path, 'network-report.md')
	# the QA network report (an external source) writes its verdict line in Portuguese: '**Veredito: …'
	if os.path.exists(rep):
		line = next((l for l in open(rep) if l.startswith('**Veredito')), '').strip().strip('*')
		verdict = (verdict + '; ' if verdict else '') + 'network: ' + line.replace('Veredito: ', '')
	bench = os.path.join(path, 'resumo.json')
	if os.path.exists(bench):
		r = json.load(open(bench)); w = r['metricas']['spawn_ate_workbench_ms']
		verdict = f"{r['validas']}/{r['n']} valid; workbench ready p50 {w['p50']} ms, p90 {w['p90']} ms"
	for cap in ('capacity-load.json', 'capacity-stress.json', 'capacity-soak.json'):
		if os.path.exists(os.path.join(path, cap)):
			verdict += f'; data in {cap}'
	rows.append((name, verdict or '(no summary)'))
with open(os.path.join(dst, 'SUMMARY.md'), 'w') as f:
	f.write('# Tests of the VSCodium 1.147 build (from 2026-11-16 on)\n\nCopies of the runs in `05-qa/results`. One folder per run; `steps.json` has each step, `screenshots/` the captures.\n\n| Run | Result |\n|---|---|\n')
	for name, verdict in rows:
		f.write(f'| `{name}` | {verdict} |\n')
	for txt in sorted(n for n in os.listdir(dst) if n.endswith('-bateria.txt')):
		f.write(f'\n## {txt}\n\n```\n{open(os.path.join(dst, txt)).read()}```\n')
print(f'   {len(rows)} runs copied')
PY

if [ "${SKIP_FORMAT:-0}" != 1 ]; then
  step "02 app JavaScript formatted (esbuild without minifying; names stay minified)"
  rm -rf "$RE/out-formatted"
  (cd "$APP/resources/app/out" && find . -name '*.js' -print0) | while IFS= read -r -d '' f; do
    mkdir -p "$RE/out-formatted/$(dirname "$f")"
    "$ESBUILD" "$APP/resources/app/out/$f" --outfile="$RE/out-formatted/$f" --log-level=error --charset=utf8 --legal-comments=inline ||
      echo "   (not formatted: $f)"
  done
fi

step "skills: .claude/skills → .opencode/skills (copy for OpenCode)"
"$DEST/sync-skills.sh"

echo "[ok] $(du -sh "$DEST" | cut -f1) in $DEST"
