# shellcheck shell=bash
# Sourced first by every script: strict mode, the C locale, the events on fd 3,
# the operations, and what more than one script needs.

set -Eeuo pipefail
shopt -s inherit_errexit nullglob
export LC_ALL=C.UTF-8

# An unexpected failure: code, file as update/script.sh, line, command. Nested
# shells report from the inside out, so the runtime reads the first line.
trap 'sm_failed $? "$LINENO" "$BASH_COMMAND"' ERR
sm_failed() {
    local file="${BASH_SOURCE[1]}" dir
    dir="${file%/*}"
    printf 'fail\t%s\t%s\t%s\t%s\n' "$1" "${dir##*/}/${file##*/}" "$2" "$3" >&3
    exit 1
}

# Runs the operation the first argument names: the function op_<name>, a dash
# in the name an underscore in the function.
operation() {
    local name="${1:-}"
    if ! declare -F "op_${name//-/_}" >/dev/null; then
        echo "no operation '${name}'" >&2
        exit 2
    fi
    shift
    "op_${name//-/_}" "$@"
}

# The phase a run is in; the sentence for it is in the operation's steps.
step() {
    printf 'step\t%s\n' "$1" >&3
}

# Every request leaving the machine, bound to https, redirects included.
fetch() {
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --connect-timeout 10 --max-time 60 "$@"
}

# The AUR helper chosen in the settings; empty for none.
aur() {
    "$SM_AUR" "$@"
}

# The helper on AUR packages, without the review and menus a run cannot answer.
aur_sync() {
    case "$SM_AUR" in
    paru) aur --skipreview "$@" ;;
    yay) aur --answerclean None --answerdiff None --answeredit None "$@" ;;
    esac
}

# The GNOME extension that shows pending updates in the top bar.
ARCH_UPDATE=arch-update@RaphaelRochet

# Its folder where it is installed, this user's before the system's.
arch_update_dir() {
    local base
    for base in "${XDG_DATA_HOME:-$HOME/.local/share}" /usr/share; do
        if [ -f "${base}/gnome-shell/extensions/${ARCH_UPDATE}/metadata.json" ]; then
            printf '%s\n' "${base}/gnome-shell/extensions/${ARCH_UPDATE}"
            return 0
        fi
    done
    return 1
}
