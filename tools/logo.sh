#!/usr/bin/env bash
# Rasters the Arch Linux mark of the package filesystem into the larger steps of
# the splash logo, in half blocks square in the window's cells, and writes
# src/ui/logo.rs. The test in src/window.rs holds each step square. Needs
# librsvg and imagemagick.
#
#   tools/logo.sh
set -euo pipefail
cd "$(dirname "$0")/.."

svg=/usr/share/pixmaps/archlinux-logo.svg
# The window's cell in pixels, Fira Code at its size as src/window.rs measures it.
cell_w=9
cell_h=23
# A third of a maximized window at 1080p and at 1440p.
steps=(13 18)

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
rsvg-convert -w 1024 -h 1024 "$svg" -o "${work}/mark.png"

# One step as Rust string literals: the mark fitted into the cells, each half
# cell filled where the mark covers at least half of it.
step() {
    local rows=$1 cols box
    cols=$(((rows * cell_h + cell_w / 2) / cell_w | 1))
    box="$((cols * cell_w * 8))x$((rows * cell_h * 8))"
    magick "${work}/mark.png" -trim +repage -alpha extract -resize "$box" \
        -background black -gravity center -extent "$box" \
        -filter box -resize "${cols}x$((rows * 2))!" -depth 8 gray:- |
        od -An -v -tu1 -w"$cols" |
        awk '
            NR % 2 { split($0, top); next }
            {
                line = ""
                for (i = 1; i <= NF; i++) {
                    t = top[i] >= 128; b = $i >= 128
                    line = line (t && b ? "█" : t ? "▀" : b ? "▄" : " ")
                }
                printf "        \"%s\",\n", line
            }'
}

{
    printf '%s\n' \
        "//! The larger steps of the splash logo, written by tools/logo.sh." \
        "" \
        "pub const STEPS: [&[&str]; ${#steps[@]}] = ["
    for rows in "${steps[@]}"; do
        printf '    &[\n'
        step "$rows"
        printf '    ],\n'
    done
    printf '];\n'
} >"${work}/logo.rs"
mv "${work}/logo.rs" src/ui/logo.rs
