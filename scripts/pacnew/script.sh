# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

# The .pacnew and .pacsave files pacman left beside a changed configuration.
op_list() {
    pacdiff --output
}

# diff ends with 1 when the files differ, 2 when one cannot be read.
op_diff() {
    local status=0
    diff --unified -- "${1%.pac*}" "$1" || status=$?
    case "$status" in
    0 | 1) ;;
    *) exit 10 ;;
    esac
}

# The new version in place of the current one.
op_take() {
    update "$1"
    sudo mv -f -- "$1" "${1%.pac*}"
}

# The current one stays, the new version goes.
op_keep() {
    update "$1"
    sudo rm -f -- "$1"
}

# Meld runs as the user on copies, so it opens on Wayland and never edits as
# root. What is saved on the left is copied into the configuration, which keeps
# its owner and mode.
op_merge() {
    local file="$1" target
    update "$file"
    target="${file%.pac*}"
    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT
    sudo install -m 0600 -o "$(id -u)" -g "$(id -g)" -- "$target" "${work}/current"
    sudo install -m 0600 -o "$(id -u)" -g "$(id -g)" -- "$file" "${work}/new"
    cp "${work}/current" "${work}/before"
    meld "${work}/current" "${work}/new"
    if cmp -s "${work}/current" "${work}/before"; then exit 12; fi
    sudo cp -- "${work}/current" "$target"
    sudo rm -f -- "$file"
}

# Nothing but a configuration update is ever moved or removed.
update() {
    case "$1" in
    *.pacnew | *.pacsave) ;;
    *) exit 11 ;;
    esac
}

operation "$@"
