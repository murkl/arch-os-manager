#!/usr/bin/env bash
# Writes a release version into the slot a built binary carries, then asks the binary.
# Usage: stamp.sh <binary> <version>
set -euo pipefail

binary="$1" version="$2"
# The slot: a NUL, the marker, then 32 bytes for the version, NUL-padded. See src/version.rs.
offsets="$(grep -obUaP '\x00SM-VERSION-SLOT' "$binary" | cut -d: -f1)" || true
if [ "$(wc -l <<<"$offsets")" -ne 1 ] || [ -z "$offsets" ]; then
    echo "stamp: $binary has not exactly one version slot" >&2
    exit 1
fi
if [ "${#version}" -ge 32 ]; then
    echo "stamp: $version does not fit the slot" >&2
    exit 1
fi
{
    printf '%s' "$version"
    printf '%*s' "$((32 - ${#version}))" '' | tr ' ' '\0'
} | dd of="$binary" bs=1 seek="$((offsets + 16))" conv=notrunc status=none

said="$("$binary" --version)"
if [ "$said" != "system-manager $version" ]; then
    echo "stamp: $binary answers '$said'" >&2
    exit 1
fi
