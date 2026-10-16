#!/usr/bin/env bash
# Creates the GitHub release of a tag, with what `make release` left in dist/.
# Usage: publish.sh <tag>; needs TOKEN, GITHUB_API_URL and GITHUB_REPOSITORY.
set -euo pipefail

tag="$1"
call() {
    curl --proto '=https' --proto-redir '=https' -fsS -H "Authorization: Bearer ${TOKEN}" \
        -H 'Accept: application/vnd.github+json' "$@"
}
body="$(jq -n --arg tag "$tag" --rawfile notes dist/notes.md '{tag_name: $tag, name: $tag, body: $notes}')"
release="$(call -X POST -d "$body" "${GITHUB_API_URL}/repos/${GITHUB_REPOSITORY}/releases")"
# The address assets go to, without its template part {?name,label}.
uploads="$(jq -er '.upload_url | sub("[{].*"; "")' <<<"$release")"
upload() {
    call -X POST -H 'Content-Type: application/octet-stream' --data-binary "@$1" "${uploads}?name=$2" >/dev/null
}
upload dist/system-manager system-manager
upload dist/system-manager.sha256 system-manager.sha256
upload dist/aur/PKGBUILD PKGBUILD
upload dist/aur/.SRCINFO SRCINFO
upload dist/aur/arch-os-manager.install arch-os-manager.install
