#!/usr/bin/env bash
# Cuts the window's icon font down to the icons src/ui/icon.rs names, out of
# the Nerd Fonts symbols release. The test in src/window.rs holds the file to
# the icons. Needs python-fonttools.
#
#   tools/icons.sh
set -euo pipefail
cd "$(dirname "$0")/.."

version=3.4.0
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl --proto '=https' --proto-redir '=https' -fsSL -o "${work}/symbols.zip" \
    "https://github.com/ryanoasis/nerd-fonts/releases/download/v${version}/NerdFontsSymbolsOnly.zip"
bsdtar -xf "${work}/symbols.zip" -C "$work" SymbolsNerdFontMono-Regular.ttf
icons="$(grep -o '\\u{[0-9a-f]*}' src/ui/icon.rs | tr -d '\\u{}' | sort -u | paste -sd,)"
pyftsubset "${work}/SymbolsNerdFontMono-Regular.ttf" --unicodes="$icons" --no-hinting \
    --output-file="assets/fonts/SymbolsNerdFontMono-${version}-Icons.ttf"
