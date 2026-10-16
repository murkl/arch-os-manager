#!/usr/bin/env bash
# Renders docs/banner.png out of two screenshots, so after `make screenshots`.
# banner.py is the same file in every project that uses it; what is this
# project's comes from here. Needs chromium, imagemagick and FiraCode Nerd Font.
#
#   docs/banner.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# The product as banner.py reads it: the built-in theme's accent, and the
# wordmark under its eyebrow.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
accent="$(sed -n 's/^ *accent: *"\(#[0-9A-Fa-f]*\)".*/\1/p' themes/nord.yaml)"
{
    printf 'accent: "%s"\nlogo: |\n' "$accent"
    sed 's/^/  /' docs/wordmark.txt
} >"${work}/product.yaml"

python3 docs/banner.py \
    --product "${work}/product.yaml" \
    --logo scripts/desktop/logo.svg \
    --card docs/screenshots/home.png \
    --card docs/screenshots/update.png \
    --tagline "Arch Linux kept current, clean and healthy, from one quiet screen." \
    --cell 7
