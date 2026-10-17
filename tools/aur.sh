#!/usr/bin/env bash
# Stamps aur/PKGBUILD for one version into a folder, with its install file and .SRCINFO, for the AUR.
# Usage: aur.sh <version> <source tarball> <folder>
set -euo pipefail

version="$1" source="$2" out="$3"
mkdir -p "$out"
source_sum="$(sha256sum "$source")"
sed -e "s/{{VERSION}}/${version}/" -e "s/{{SHA256}}/${source_sum%% *}/" aur/PKGBUILD >"${out}/PKGBUILD"
if grep -n '{{' "${out}/PKGBUILD" >&2; then
    echo "aur: a placeholder is left" >&2
    exit 1
fi
cp aur/arch-os-manager.install "$out"

# Read back by makepkg itself, which refuses root, as a CI container is.
if [ "$(id -u)" -eq 0 ]; then
    chmod 755 "$out"
    (cd "$out" && runuser -u nobody -- env BUILDDIR=/tmp PKGDEST=/tmp SRCDEST=/tmp SRCPKGDEST=/tmp LOGDEST=/tmp makepkg --printsrcinfo) >"${out}/.SRCINFO"
else
    (cd "$out" && makepkg --printsrcinfo) >"${out}/.SRCINFO"
fi
grep -qx "	pkgver = ${version}" "${out}/.SRCINFO"
