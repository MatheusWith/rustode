#!/usr/bin/env bash
# Monta ou atualiza este projeto a partir das fontes (tudo é CÓPIA real: sem links simbólicos nem físicos;
# as fontes não são alteradas).
# Idempotente: rode de novo depois de novos testes para trazer resultados, fotos e logs recentes.
# Uso: ./montar.sh            tudo
#      SEM_FORMATADO=1 ...    pula as cópias formatadas do JavaScript (a parte mais lenta)
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
RE=$DEST/02-engenharia-reversa
step() { echo ">> $*"; }

step "01 app gerado (alvo do build)"
mkdir -p "$DEST/01-app"
rsync -a --copy-links --delete "$APP/" "$DEST/01-app/VSCode-linux-x64/"

step "05 QA (documentos, suíte, resultados, histórico git)"
rsync -a --copy-links --delete "$QA/" "$DEST/05-qa/"

step "06 port (scripts, patches originais e adaptados, logs de build)"
rsync -a --copy-links --delete "$PORT/" "$DEST/06-port/"

step "07 VSCodium de origem (referência: patches, docs, scripts, product.json)"
mkdir -p "$DEST/07-vscodium-origem"
rsync -a --copy-links --delete --include='/patches/***' --include='/docs/***' --include='/src/***' --include='/*.sh' \
  --include='/product.json' --include='/announcements-*.json' --include='/LICENSE' --include='/README.md' --exclude='*' \
  "$VSCODIUM/" "$DEST/07-vscodium-origem/"
git -C "$VSCODIUM" log -1 --format='%H %cs %s' >"$DEST/07-vscodium-origem/COMMIT.txt"

step "03 documentos (cópias independentes)"
rm -rf "$DEST/03-documentos"
mkdir -p "$DEST/03-documentos"
cp -a "$DEST/05-qa/docs" "$DEST/03-documentos/qa"
cp -a "$DEST/06-port/PORT-NOTES.md" "$DEST/03-documentos/PORT-NOTES.md"
cp -a "$DEST/07-vscodium-origem/docs" "$DEST/03-documentos/vscodium-docs"
for f in "$DEST/05-qa/results"/*/network-report.md; do
  if [ -f "$f" ]; then cp -a "$f" "$DEST/03-documentos/rede-$(basename "$(dirname "$f")").md"; fi
done

step "04 fotos (capturas dos testes, cópias independentes, mesma árvore de pastas)"
rm -rf "$DEST/04-fotos"
mkdir -p "$DEST/04-fotos"
(cd "$DEST/05-qa/results" && find . -type f \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' -o -iname '*.webp' \) -print0 |
  xargs -0 -r cp -a --parents -t "$DEST/04-fotos")

step "02: metadados, manifesto, binários"
mkdir -p "$RE/asar" "$RE/fonte"
cp "$APP/resources/app/product.json" "$APP/resources/app/package.json" "$RE/"
(cd "$DEST/01-app/VSCode-linux-x64" && find . -type f -print0 | sort -z | xargs -0 sha256sum) >"$RE/manifesto-sha256.txt"
(cd "$DEST/01-app/VSCode-linux-x64" && find . -type f \( -name '*.node' -o -name '*.so*' -o -perm -u+x \) -print0 | sort -z | xargs -0 file) >"$RE/binarios.txt"
"$APP/bin/codium" --version >"$RE/versao.txt" 2>&1 || true

step "02 asar: lista e extração do node_modules.asar"
"$NODE" "$ASAR" list "$APP/resources/app/node_modules.asar" >"$RE/asar/node_modules.asar.lista.txt"
rm -rf "$RE/asar/node_modules.asar.extraido"
"$NODE" "$ASAR" extract "$APP/resources/app/node_modules.asar" "$RE/asar/node_modules.asar.extraido"

step "02 fonte exata do build (commit + patches + passos pós-instalação), sem node_modules nem saídas"
TMPL=$(mktemp)
(cd "$WT" && git ls-files -co --exclude-standard -z) >"$TMPL"
# --ignore-missing-args: o git ainda lista arquivos rastreados que os patches do port removeram da worktree
# --copy-links: o único link do repositório (fixture de teste do terminal-suggest) vira arquivo real
rsync -a --copy-links --delete --ignore-missing-args --from0 --files-from="$TMPL" "$WT/" "$RE/fonte/arvore/"
rm -f "$TMPL"
IDX=$(mktemp -u)
TREE=$(cd "$WT" && GIT_INDEX_FILE=$IDX git read-tree HEAD && GIT_INDEX_FILE=$IDX git add -A 2>/dev/null && GIT_INDEX_FILE=$IDX git write-tree)
rm -f "$IDX"
# --irreversible-delete: arquivos removidos (ex.: extensions/copilot) aparecem só como removidos, sem o conteúdo
(cd "$WT" && git diff --binary --irreversible-delete HEAD "$TREE") >"$RE/fonte/port-aplicado.diff"
(cd "$WT" && git diff --stat=200 --irreversible-delete HEAD "$TREE" | tail -1) >"$RE/fonte/port-aplicado.stat.txt"
{
  echo "VS Code (base):   $(git -C "$WT" rev-parse HEAD)  ($(git -C "$WT" log -1 --format='%cs %s' | cut -c1-100))"
  echo "VSCodium (origem): $(cat "$DEST/07-vscodium-origem/COMMIT.txt" | cut -c1-120)"
  echo "Versão do app:     $(head -1 "$RE/versao.txt")"
  echo "Electron:          $(grep -oE 'electron-v[0-9.]+' "$WT/build/checksums/electron.txt" | head -1)"
  echo "Árvore do build:   $TREE (git, objeto no repositório de $WT)"
  echo "Montado em:        $(date '+%F %T')"
} >"$RE/fonte/origem.txt"

step "08 resultados dos testes do build (cópias das execuções a partir de 2026-11-16) e RESUMO.md"
rm -rf "$DEST/08-testes-do-build"; mkdir -p "$DEST/08-testes-do-build"
python3 -I - "$QA/results" "$DEST/08-testes-do-build" <<'PY'
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
	if os.path.exists(rep):
		line = next((l for l in open(rep) if l.startswith('**Veredito')), '').strip().strip('*')
		verdict = (verdict + '; ' if verdict else '') + 'rede: ' + line.replace('Veredito: ', '')
	bench = os.path.join(path, 'resumo.json')
	if os.path.exists(bench):
		r = json.load(open(bench)); w = r['metricas']['spawn_ate_workbench_ms']
		verdict = f"{r['validas']}/{r['n']} válidas; workbench pronto p50 {w['p50']} ms, p90 {w['p90']} ms"
	for cap in ('capacity-load.json', 'capacity-stress.json', 'capacity-soak.json'):
		if os.path.exists(os.path.join(path, cap)):
			verdict += f'; dados em {cap}'
	rows.append((name, verdict or '(sem resumo)'))
with open(os.path.join(dst, 'RESUMO.md'), 'w') as f:
	f.write('# Testes do build VSCodium 1.147 (a partir de 2026-11-16)\n\nCópias das execuções de `05-qa/results`. Um diretório por execução; `steps.json` tem cada passo, `screenshots/` as capturas.\n\n| Execução | Resultado |\n|---|---|\n')
	for name, verdict in rows:
		f.write(f'| `{name}` | {verdict} |\n')
	for txt in sorted(n for n in os.listdir(dst) if n.endswith('-bateria.txt')):
		f.write(f'\n## {txt}\n\n```\n{open(os.path.join(dst, txt)).read()}```\n')
print(f'   {len(rows)} execuções copiadas')
PY

if [ "${SEM_FORMATADO:-0}" != 1 ]; then
  step "02 JavaScript do app formatado (esbuild sem minificar; nomes continuam minificados)"
  rm -rf "$RE/out-formatado"
  (cd "$APP/resources/app/out" && find . -name '*.js' -print0) | while IFS= read -r -d '' f; do
    mkdir -p "$RE/out-formatado/$(dirname "$f")"
    "$ESBUILD" "$APP/resources/app/out/$f" --outfile="$RE/out-formatado/$f" --log-level=error --charset=utf8 --legal-comments=inline ||
      echo "   (não formatado: $f)"
  done
fi

step "skills: .claude/skills → .opencode/skills (cópia para o OpenCode)"
"$DEST/sincronizar-skills.sh"

echo "[ok] $(du -sh "$DEST" | cut -f1) em $DEST"
