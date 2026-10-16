#!/usr/bin/env bash
# Prints what CHANGELOG.md says about one version; a version without entries is an error.
# Usage: notes.sh <version>
set -euo pipefail

notes="$(awk -v v="## $1" '$0 == v { on = 1; next } /^## / { on = 0 } on' CHANGELOG.md | sed '/./,$!d')"
if [ -z "$(tr -d '[:space:]' <<<"$notes")" ]; then
    echo "notes: CHANGELOG.md has no entries for $1" >&2
    exit 1
fi
printf '%s\n' "$notes"
