#!/usr/bin/env bash
# Copia as skills de .claude/skills/ (o original, lido pelo Claude Code) para .opencode/skills/ (lido pelo OpenCode).
# Cópias reais, sem links. Edite só em .claude/skills/: o que for mudado direto em .opencode/skills/ é sobrescrito.
# Uso: ./sincronizar-skills.sh          (o montar.sh também chama este script no fim)
set -euo pipefail
DEST=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$DEST/.opencode/skills"
rsync -a --copy-links --delete "$DEST/.claude/skills/" "$DEST/.opencode/skills/"
if diff -rq "$DEST/.claude/skills" "$DEST/.opencode/skills" > /dev/null; then
	echo "[ok] .opencode/skills = .claude/skills ($(find "$DEST/.opencode/skills" -name SKILL.md | wc -l) skills)"
else
	echo "[ERRO] .opencode/skills ficou diferente de .claude/skills" >&2; exit 1
fi
