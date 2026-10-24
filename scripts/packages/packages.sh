# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

# The AUR's own interface, without a helper. https://wiki.archlinux.org/title/Aurweb_RPC_interface
rpc=https://aur.archlinux.org/rpc/

# Three lists, joined by the runtime: every package with its version, the
# explicit ones and the foreign ones. pacman ends with 1 when a list is empty.
op_installed() {
    pacman -Q | sed 's/^/pkg\t/; s/ /\t/'
    pacman -Qqe | sed 's/^/explicit\t/' || allow 1
    pacman -Qqm | sed 's/^/foreign\t/' || allow 1
}

# A regular expression, escaped by the caller. pacman ends with 1 when nothing matches.
op_search() {
    pacman -Ss -- "$1" || allow 1
}

op_aur_search() {
    fetch -G --data-urlencode v=5 --data-urlencode type=search --data-urlencode by=name-desc \
        --data-urlencode "arg=$1" "$rpc" || exit 10
    echo
}

# A hundred names per request keep the address below the server's length limit.
op_aur_info() {
    local args package
    while [ $# -gt 0 ]; do
        args=(-G --data-urlencode v=5 --data-urlencode type=info)
        for package in "${@:1:100}"; do args+=(--data-urlencode "arg[]=$package"); done
        shift $(($# < 100 ? $# : 100))
        fetch "${args[@]}" "$rpc" || exit 10
        echo
    done
}

# sync <repo/name> reads a repository, local <name> the installed package.
op_info() {
    case "$1" in
    sync) pacman -Si -- "$2" || exit 11 ;;
    local) pacman -Qi -- "$2" || exit 11 ;;
    esac
}

# Every version of a package at hand: the files in the package cache, then the
# Arch Linux Archive, which keeps every build of the official repositories.
op_versions() {
    local name="$1" dir file url page status=0
    while read -r dir; do
        for file in "${dir%/}/$name"-*.pkg.tar.*; do
            if [[ $file != *.sig ]]; then printf 'cache\t%s\n' "$file"; fi
        done
    done < <(pacman-conf CacheDir)

    # Not found means the package was never in an official repository.
    url="https://archive.archlinux.org/packages/${name:0:1}/${name}/"
    page="$(fetch "$url")" || status=$?
    case "$status" in
    0) ;;
    22) return 0 ;;
    *) exit 12 ;;
    esac
    grep -o 'href="[^"]*\.pkg\.tar\.[a-z]*"' <<<"$page" | while read -r href; do
        href="${href#href=\"}"
        printf 'archive\t%s%s\n' "$url" "${href%\"}"
    done || allow 1
}

op_pkgbuild() {
    fetch -G --data-urlencode "h=$1" https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD || exit 10
}

# repo or aur, then the packages. An AUR package is named with its source, as a
# helper takes a repository package that provides the same name first.
op_install() {
    local source="$1"
    shift
    if behind; then exit 13; fi
    case "$source" in
    repo) sudo pacman -S --needed -- "$@" ;;
    aur) aur_sync -S -- "${@/#/aur/}" ;;
    esac
}

# With the dependencies nothing else needs and without saving the configuration.
op_remove() {
    sudo pacman -Rns -- "$@"
}

# repo:<repo/name>, aur:<name> and remove:<name>. Installed first, so a package
# that replaces another takes it out in its own transaction or provides what
# depends on it.
op_change() {
    local arg name installed
    local -a repo=() from_aur=() remove=() left=()
    for arg; do
        case "$arg" in
        repo:*) repo+=("${arg#repo:}") ;;
        aur:*) from_aur+=("${arg#aur:}") ;;
        remove:*) remove+=("${arg#remove:}") ;;
        esac
    done
    if [ $((${#repo[@]} + ${#from_aur[@]})) -gt 0 ] && behind; then exit 13; fi
    if [ "${#repo[@]}" -gt 0 ]; then sudo pacman -S --needed -- "${repo[@]}"; fi
    if [ "${#from_aur[@]}" -gt 0 ]; then aur_sync -S -- "${from_aur[@]/#/aur/}"; fi
    # What the installs replaced is gone. pacman -Q would still find its name in
    # what provides it, pacman -R would not.
    installed="$(pacman -Qq)"
    for name in "${remove[@]}"; do
        if grep -qFx -- "$name" <<<"$installed"; then left+=("$name"); fi
    done
    if [ "${#left[@]}" -gt 0 ]; then sudo pacman -Rns -- "${left[@]}"; fi
}

# A package file from the cache or an address in the Arch Linux Archive, and pacman
# verifies the signature of either.
op_downgrade() {
    sudo pacman -U -- "$1"
}

operation "$@"
