#!/usr/bin/env bash
# Copies the skills from .claude/skills/ (the original, read by Claude Code) to .opencode/skills/ (read by OpenCode).
# Real copies, no links. Edit only in .claude/skills/: whatever is changed directly in .opencode/skills/ is overwritten.
# Usage: ./sync-skills.sh          (assemble.sh also calls this script at the end)
set -euo pipefail
DEST=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$DEST/.opencode/skills"
rsync -a --copy-links --delete "$DEST/.claude/skills/" "$DEST/.opencode/skills/"
if diff -rq "$DEST/.claude/skills" "$DEST/.opencode/skills" > /dev/null; then
	echo "[ok] .opencode/skills = .claude/skills ($(find "$DEST/.opencode/skills" -name SKILL.md | wc -l) skills)"
else
	echo "[ERROR] .opencode/skills differs from .claude/skills" >&2; exit 1
fi
