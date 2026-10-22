# shellcheck shell=bash
# Sourced first by every script: strict mode, the C locale, the events on fd 3,
# the operations, and what more than one script needs.

set -Eeuo pipefail
shopt -s inherit_errexit nullglob
export LC_ALL=C.UTF-8

# An unexpected failure: code, file as upgrade/upgrade.sh, line, command. Nested
# shells report from the inside out, so the runtime reads the first line. Quiet
# on stderr, where the terminal would echo it as a program.
trap '{ sm_failed $? "$LINENO" "$BASH_COMMAND"; } 2>/dev/null' ERR
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
    if asks; then sm_terminal; fi
    "op_${name//-/_}" "$@"
}

# Whether the tools ask, the person answering in the terminal. Else a script
# spares each question with the tool's flag:
# `if asks; then yes=(); else yes=(--noconfirm); fi`.
asks() {
    [ "${SM_TERMINAL:-}" = on ]
}

# A closed terminal stops no run: sudo, on a terminal of its own by default,
# passes pacman no hangup. A run, which $SM_RUN names, holds its run_lock and,
# where logind answers, shutdown and sleep off. Each through a process that
# ends with the script, so nothing the run starts, such as a build server,
# keeps it. The run lock is shared, so runs never wait for each other.
sm_hold() {
    local lock held
    trap '' HUP
    if [ -z "${SM_RUN:-}" ]; then return 0; fi
    if lock="$(run_lock)"; then
        exec {held}>>"$lock"
        flock --shared "$held"
        tail --pid=$$ -f /dev/null </dev/null >/dev/null 2>&1 3>&- &
        exec {held}>&-
    fi
    if ! systemd-inhibit --list >/dev/null 2>&1; then return 0; fi
    systemd-inhibit --what=shutdown:sleep --mode=block --who="System Manager" --why="$SM_RUN" \
        tail --pid=$$ -f /dev/null </dev/null >/dev/null 2>&1 3>&- &
}

# The lock a run of this script holds while it lasts, for a query to wait on.
# In this user's runtime folder, which every session has and only this user
# writes. Without one of its own, as for root through sudo, there is none.
run_lock() {
    local dir="${BASH_SOURCE[-1]%/*}"
    if ! [ -O "${XDG_RUNTIME_DIR:-}" ]; then return 1; fi
    echo "${XDG_RUNTIME_DIR}/system-manager-${dir##*/}.lock"
}

# In the terminal the person sees every program before it runs, as
# `$ sudo pacman -Syu`: xtrace, which sm_trace turns on for a program alone.
# The space first, as bash repeats the first character of PS4 for a subshell.
sm_terminal() {
    PS4=$' \e[90m$\e[0m '
    set -o functrace
    trap '{ sm_trace; } 2>/dev/null' DEBUG
}

# Before every command: xtrace on for a program on the PATH, off for a builtin, a
# function or the script's own logic. A program named through one variable, as
# the AUR helper is, is read through it.
sm_trace() {
    local word="${BASH_COMMAND%% *}" name
    if [[ $word =~ ^\"\$([A-Za-z_][A-Za-z0-9_]*)\"$ ]]; then
        name="${BASH_REMATCH[1]}"
        word="${!name:-}"
    fi
    if [ "$(type -t "$word" || true)" = file ]; then set -x; else set +x; fi
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

# Every request leaving the machine, bound to the protocol of its address,
# redirects included: https stays https, the plain http of a server at home
# stays at home.
fetch() {
    local url="${*: -1}"
    local proto="${url%%://*}"
    curl --proto "=${proto}" --proto-redir "=${proto}" --tlsv1.2 -fsSL --connect-timeout 10 --max-time 60 "$@"
}

# The AUR helper chosen in the settings, empty for none.
aur() {
    "$SM_AUR" "$@"
}

# The helper on AUR packages, without the review and menus a run cannot answer.
# In the terminal the person answers them.
aur_sync() {
    if asks; then
        aur "$@"
        return
    fi
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

# An extension of extensions.gnome.org in the version for this GNOME Shell,
# installed for this user. GNOME loads one it did not start with at the next login.
ego_install() {
    local shell info url zip status=0
    shell="$(gnome-shell --version)"
    shell="${shell##* }"
    info="$(fetch "https://extensions.gnome.org/extension-info/?uuid=$1&shell_version=${shell%%.*}")" || return 1
    url="$(sed -n 's/.*"download_url": *"\([^"]*\)".*/\1/p' <<<"$info")"
    if [ -z "$url" ]; then return 1; fi
    zip="$(mktemp --suffix=.zip)"
    fetch "https://extensions.gnome.org${url}" >"$zip" || status=1
    if [ "$status" = 0 ]; then gnome-extensions install --force "$zip"; fi
    rm -f -- "$zip"
    return "$status"
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
