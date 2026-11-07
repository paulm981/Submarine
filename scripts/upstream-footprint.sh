#!/usr/bin/env bash
# How many lines the fork changes inside the original project's own files:
# what can conflict at a sync (CLAUDE.md, "Fork features that don't cost
# sync time"). Files only the fork has cost nothing at a sync and aren't
# counted. Compares REF (default HEAD) with upstream/main.
#
#   git fetch upstream && scripts/upstream-footprint.sh [REF]
set -euo pipefail
ref="${1:-HEAD}"
git rev-parse -q --verify upstream/main >/dev/null || { echo "No upstream/main: git remote add upstream https://github.com/SinaXhpm/submarine.git && git fetch upstream" >&2; exit 1; }
files=()
while IFS= read -r f; do
  git cat-file -e "upstream/main:$f" 2>/dev/null && files+=("$f")
done < <(git diff --name-only upstream/main "$ref")
if [ ${#files[@]} -eq 0 ]; then echo "0 lines in 0 of the original project's files"; exit 0; fi
git diff --numstat upstream/main "$ref" -- "${files[@]}" \
  | awk '$1 != "-" { n += $1 + $2; f++; print $1 + $2 "\t" $3 } END { print n " lines in " f " of the original project'"'"'s files" > "/dev/stderr" }' \
  | sort -rn | head -10
