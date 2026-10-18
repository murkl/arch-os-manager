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
    local frame=1 line="$2" file dir
    # Within a helper of this file, the line is the script's that called it.
    while [[ ${BASH_SOURCE[frame]} == */lib.sh && -n ${BASH_SOURCE[frame + 1]:-} ]]; do
        line="${BASH_LINENO[frame]}"
        frame=$((frame + 1))
    done
    file="${BASH_SOURCE[frame]}"
    dir="${file%/*}"
    printf 'fail\t%s\t%s\t%s\t%s\n' "$1" "${dir##*/}/${file##*/}" "$line" "$3" >&3
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
    sm_hold
    "op_${name//-/_}" "$@"
}

# A closed terminal stops no run: sudo, on a terminal of its own by default,
# passes pacman no hangup. A run, which $SM_RUN names, holds shutdown and sleep
# off where logind answers, through a lock that ends with the script.
sm_hold() {
    trap '' HUP
    if [ -z "${SM_RUN:-}" ] || ! systemd-inhibit --list >/dev/null 2>&1; then return 0; fi
    systemd-inhibit --what=shutdown:sleep --mode=block --who="System Manager" --why="$SM_RUN" \
        tail --pid=$$ -f /dev/null </dev/null >/dev/null 2>&1 3>&- &
}

# The phase a run is in. The sentence for it is in the operation's steps.
step() {
    printf 'step\t%s\n' "$1" >&3
}

# The status of the command before, unless it is one of the codes named, by
# which that command says nothing went wrong: `cmd || allow 1`.
allow() {
    local status=$?
    for code; do
        if [ "$status" -eq "$code" ]; then return 0; fi
    done
    return "$status"
}

# A folder of the operation's own as $work, removed when it ends.
scratch() {
    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT
}

# Every request leaving the machine, bound to https, redirects included.
fetch() {
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --connect-timeout 10 --max-time 60 "$@"
}

# The AUR helper chosen in the settings, empty for none.
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

# Runs what reads or writes GSettings on the session bus, or where none answers,
# as in an installer's chroot, on a private one, which dconf writes to this
# user's database all the same. `man dbus-run-session`
on_bus() {
    if [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ] || [ -S "${XDG_RUNTIME_DIR:-}/bus" ]; then
        "$@"
    else
        dbus-run-session -- "$@"
    fi
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
