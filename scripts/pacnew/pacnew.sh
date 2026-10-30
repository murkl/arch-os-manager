# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

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

# The new version in place of the current one. An account database or sudoers
# as packaged locks the people out who are in the own one, so it is merged only.
op_take() {
    update "$1"
    case "${1%.pac*}" in
    /etc/passwd | /etc/group | /etc/shadow | /etc/gshadow | /etc/sudoers) exit 14 ;;
    esac
    if snapshots; then
        step snapshot
        sudo snapper -c root create --cleanup-algorithm number --description "system-manager: ${1%.pac*}"
    fi
    sudo mv -f -- "$1" "${1%.pac*}"
}

# The current one stays, the new version goes.
op_keep() {
    update "$1"
    sudo rm -f -- "$1"
}

# Meld runs as the user on copies, so it opens on Wayland and never edits as
# root, in the person's language. What is saved on the left goes into the
# configuration, which keeps its owner and mode.
op_merge() {
    local file="$1" target meta mode owner group
    update "$file"
    target="${file%.pac*}"
    if snapshots; then
        step snapshot
        sudo snapper -c root create --cleanup-algorithm number --description "system-manager: $target"
    fi
    scratch
    sudo install -m 0600 -o "$(id -u)" -g "$(id -g)" -- "$target" "${work}/current"
    sudo install -m 0600 -o "$(id -u)" -g "$(id -g)" -- "$file" "${work}/new"
    cp "${work}/current" "${work}/before"
    env -u LC_ALL meld "${work}/current" "${work}/new"
    if cmp -s "${work}/current" "${work}/before"; then exit 12; fi
    # sudo refuses to start with a sudoers that does not parse.
    case "$target" in
    */sudoers | */sudoers.d/*) sudo visudo --check --file "${work}/current" >/dev/null || exit 13 ;;
    esac
    # Written beside it with its owner and mode, then renamed over it, so nothing
    # reads half a configuration. Its folder may be closed to the user. A link
    # gives the owner and mode of its target, its own mode is 777.
    meta="$(sudo stat -L -c '%a %u %g' -- "$target")"
    read -r mode owner group <<<"$meta"
    sudo install -m "$mode" -o "$owner" -g "$group" -- "${work}/current" "${target}.sm-new"
    sudo mv -f -- "${target}.sm-new" "$target"
    sudo rm -f -- "$file"
}

# Where snapper keeps the root, a snapshot holds the configuration before it
# changes, pruned by snapper's own number limit, so nothing is left beside it.
snapshots() {
    command -v snapper >/dev/null && [ -e /etc/snapper/configs/root ]
}

# Nothing but what pacdiff lists right now is ever moved or removed.
update() {
    local listed
    listed="$(pacdiff --output)"
    grep -qxF -- "$1" <<<"$listed" || exit 11
}

operation "$@"
