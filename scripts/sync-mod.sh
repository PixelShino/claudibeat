#!/bin/sh
# Copy the status-band mod from where it runs (~/.claude/mods/status-band) into
# this repo's mod/status-band, then check the copy for anything private.
# The mod's own README is Russian; here it becomes README.ru.md beside the
# English README.md kept in this repo.
set -e
src="${1:-$HOME/.claude/mods/status-band}"
root=$(cd "$(dirname "$0")/.." && pwd)
dst="$root/mod/status-band"

[ -f "$src/.claude-plugin/plugin.json" ] || { echo "no mod at $src" >&2; exit 1; }
keep="$dst/README.md"
tmp=$(mktemp)
[ -f "$keep" ] && cp "$keep" "$tmp"
rm -rf "$dst"
mkdir -p "$dst"
(cd "$src" && tar --exclude=node_modules --exclude=.git -cf - .) | (cd "$dst" && tar -xf -)
mv "$dst/README.md" "$dst/README.ru.md"
[ -s "$tmp" ] && cp "$tmp" "$keep"
rm -f "$tmp"

cd "$root"
git add mod/status-band
sh .githooks/pre-commit
echo "mod copied to mod/status-band and staged"
