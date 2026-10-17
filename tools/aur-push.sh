#!/usr/bin/env bash
# Pushes a folder tools/aur.sh stamped to the AUR repository of its package,
# in place of what is there, as the PKGBUILD's maintainer.
# Usage: aur-push.sh <folder>
set -euo pipefail

dir="$(cd "$1" && pwd)"
name="$(sed -n 's/^pkgbase = //p' "${dir}/.SRCINFO")"
version="$(sed -n 's/^\tpkgver = //p' "${dir}/.SRCINFO")"
install="$(sed -n 's/^\tinstall = //p' "${dir}/.SRCINFO")"
maintainer="$(sed -n 's/^# Maintainer: \(.* <.*>\)$/\1/p' "${dir}/PKGBUILD")"
if [ -z "$name" ] || [ -z "$version" ] || [ -z "$maintainer" ]; then
    echo "aur-push: ${dir} holds no stamped package" >&2
    exit 1
fi
email="${maintainer##*<}"
export GIT_AUTHOR_NAME="${maintainer% <*}" GIT_AUTHOR_EMAIL="${email%>}"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME" GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
git clone --quiet "ssh://aur@aur.archlinux.org/${name}.git" "$work"
cd "$work"
# What is there goes first, so a file the release no longer has goes with it.
git rm -q -r --ignore-unmatch -- .
cp "${dir}/PKGBUILD" "${dir}/.SRCINFO" .
if [ -n "$install" ]; then cp "${dir}/${install}" .; fi
git add -A
if git diff --cached --quiet; then
    echo "aur-push: ${name} ${version} is there already"
    exit 0
fi
git commit --quiet -m "$version"
git push --quiet origin HEAD:master
echo "aur-push: ${name} ${version}"
