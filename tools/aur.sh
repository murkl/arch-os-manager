#!/usr/bin/env bash
# The AUR package out of aur/: stamped for a version, built as a user builds it,
# pushed as the PKGBUILD's maintainer, and its binary put on the release. A tag
# is its version, as the PKGBUILD's source names it.
#
#   tools/aur.sh stamp <version> <source tarball> <folder>
#   tools/aur.sh build <version> <folder>
#   tools/aur.sh push <folder>               AUR_SSH_KEY, else the user's ssh
#   tools/aur.sh upload <version> <folder>   gh, logged in or GH_TOKEN
#   tools/aur.sh key                         gh as an admin, by hand
set -euo pipefail

# A folder of the command's own, global so the trap that removes it still sees it.
work=""

# Every file of aur/, the PKGBUILD's placeholders filled, and .SRCINFO.
stamp() {
    local version="$1" source="$2" out="$3" source_sum
    mkdir -p "$out"
    source_sum="$(sha256sum "$source")"
    cp aur/* "$out"
    sed -i -e "s/{{VERSION}}/${version}/" -e "s/{{SHA256}}/${source_sum%% *}/" "${out}/PKGBUILD"
    if grep -n '{{' "${out}/PKGBUILD" >&2; then
        echo "aur: a placeholder is left" >&2
        exit 1
    fi

    # Read back by makepkg itself, which refuses root, as a CI container is.
    if [ "$(id -u)" -eq 0 ]; then
        chmod 755 "$out"
        (cd "$out" && runuser -u nobody -- env BUILDDIR=/tmp PKGDEST=/tmp SRCDEST=/tmp SRCPKGDEST=/tmp LOGDEST=/tmp makepkg --printsrcinfo) >"${out}/.SRCINFO"
    else
        (cd "$out" && makepkg --printsrcinfo) >"${out}/.SRCINFO"
    fi
    grep -qx "	pkgver = ${version}" "${out}/.SRCINFO"
}

# The tarball lies beside the PKGBUILD, where makepkg looks first and checks it
# against the sum. As root, the container's, makepkg runs as a builder with a
# home of its own for cargo, on a copy: the build runs the code of every crate,
# and what root later pushes with the key stays out of its reach. What it
# started ends with it, and only the package comes back, never a link.
build() {
    local version="$1" out="$2" name url tar needed
    name="$(sed -n 's/^pkgname=//p' aur/PKGBUILD)"
    url="$(sed -n "s/^url='\(.*\)'$/\1/p" aur/PKGBUILD)"
    tar="${out}/${name}-${version}.tar.gz"
    mkdir -p "$out"
    curl --proto '=https' --proto-redir '=https' -fsSL -o "$tar" "${url}/archive/refs/tags/${version}.tar.gz"
    stamp "$version" "$tar" "$out"
    if [ "$(id -u)" -eq 0 ]; then
        # makepkg builds only where its dependencies are, which a helper installs first.
        mapfile -t needed < <(sed -n 's/^\t\(make\)\?depends = //p' "${out}/.SRCINFO")
        pacman -S --needed --noconfirm -- "${needed[@]}"
        useradd --create-home builder
        work="$(mktemp -d)"
        trap 'rm -rf "$work"' EXIT
        cp -r "$out" "${work}/package"
        chown -R builder "$work"
        (cd "${work}/package" && runuser -u builder -- env HOME=/home/builder makepkg --noconfirm)
        pkill -KILL -u builder || [ "$?" -eq 1 ]
        find "${work}/package" -maxdepth 1 -type f -name "${name}-${version}-*-x86_64.pkg.tar.zst" \
            -exec install -m 644 -t "$out" -- {} +
    else
        (cd "$out" && makepkg --noconfirm)
    fi
}

# A stamped folder in place of what the AUR has. The host key is the one whose
# fingerprint https://aur.archlinux.org names.
push() {
    local dir name version install maintainer email there
    dir="$(cd "$1" && pwd)"
    name="$(sed -n 's/^pkgbase = //p' "${dir}/.SRCINFO")"
    version="$(sed -n 's/^\tpkgver = //p' "${dir}/.SRCINFO")"
    install="$(sed -n 's/^\tinstall = //p' "${dir}/.SRCINFO")"
    maintainer="$(sed -n 's/^# Maintainer: \(.* <.*>\)$/\1/p' "${dir}/PKGBUILD")"
    if [ -z "$name" ] || [ -z "$version" ] || [ -z "$maintainer" ]; then
        echo "aur: ${dir} holds no stamped package" >&2
        exit 1
    fi
    email="${maintainer##*<}"
    export GIT_AUTHOR_NAME="${maintainer% <*}" GIT_AUTHOR_EMAIL="${email%>}"
    export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME" GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"

    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT
    if [ -n "${AUR_SSH_KEY:-}" ]; then
        mkdir -m 700 "${work}/ssh"
        printf '%s\n' "$AUR_SSH_KEY" >"${work}/ssh/key"
        chmod 600 "${work}/ssh/key"
        echo 'aur.archlinux.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEuBKrPzbawxA/k2g6NcyV5jmqwJ2s+zpgZGZ7tpLIcN' >"${work}/ssh/known_hosts"
        export GIT_SSH_COMMAND="ssh -i ${work}/ssh/key -o IdentitiesOnly=yes -o UserKnownHostsFile=${work}/ssh/known_hosts -o StrictHostKeyChecking=yes"
    fi
    git clone --quiet "ssh://aur@aur.archlinux.org/${name}.git" "${work}/repo"
    cd "${work}/repo"
    # A newer version there stays, as after a rerun for an older tag.
    if [ -f .SRCINFO ]; then
        there="$(sed -n 's/^\tpkgver = //p' .SRCINFO)"
        if [ -n "$there" ] && [ "$(vercmp "$version" "$there")" -lt 0 ]; then
            echo "aur: ${name} ${there} is newer than ${version}"
            return 0
        fi
    fi
    # What is there goes first, so a file the release no longer has goes with it.
    git rm -q -r --ignore-unmatch -- .
    cp "${dir}/PKGBUILD" "${dir}/.SRCINFO" .
    if [ -n "$install" ]; then cp "${dir}/${install}" .; fi
    git add -A
    if git diff --cached --quiet; then
        echo "aur: ${name} ${version} is there already"
        return 0
    fi
    git commit --quiet -m "$version"
    git push --quiet origin HEAD:master
    echo "aur: ${name} ${version}"
}

# The same binary the package installs. GitHub shows its sha256 beside it.
upload() {
    local version="$1" out="$2" name bin
    name="$(sed -n 's/^pkgname=//p' aur/PKGBUILD)"
    bin="${out}/system-manager-${version}-x86_64"
    bsdtar -xOf "${out}/${name}-${version}"-*-x86_64.pkg.tar.zst usr/bin/system-manager >"$bin"
    gh release upload "$version" "$bin" --clobber
}

# The release's own key for the AUR: made here, kept only as the secret
# AUR_SSH_KEY of the environment aur, which version tags alone may use. Prints
# the public half for the AUR account's SSH Public Key field, as one more line
# there.
key() {
    local repo
    repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT
    ssh-keygen -q -t ed25519 -N '' -C "${repo} release" -f "${work}/key"

    gh api --method PUT "repos/${repo}/environments/aur" --input - >/dev/null <<'JSON'
{"deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}
JSON
    # One that exists already answers 303, and gh follows it there.
    gh api --method POST "repos/${repo}/environments/aur/deployment-branch-policies" \
        -f name="*.*.*" -f type=tag >/dev/null
    gh secret set AUR_SSH_KEY --env aur --repo "$repo" <"${work}/key"

    echo "Add this line at https://aur.archlinux.org/account/ under SSH Public Key:"
    cat "${work}/key.pub"
}

case "${1:-}" in
stamp | build | push | upload | key) "$@" ;;
*)
    echo "usage: tools/aur.sh stamp|build|push|upload|key" >&2
    exit 2
    ;;
esac
