# shellcheck shell=bash
# Sourced first by every script: strict mode, the C locale, the events on fd 3,
# the operations, and what more than one script needs.

set -Eeuo pipefail
shopt -s inherit_errexit nullglob
export LC_ALL=C.UTF-8

# An unexpected failure: code, file as upgrade/upgrade.sh, line, command, quiet
# on stderr, where the terminal would echo it as a program. Nested shells report
# from the inside out, so the runtime reads the first line.
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

# What the script made for its own use, removed as it ends, also on an error or
# ctrl+c: the folder of scratch, and each file written beside its target until
# it takes the target's place.
sm_made=()
# The same of root, which sudo removes without asking: the run used it just
# before.
sm_made_root=()
trap sm_clean EXIT
sm_clean() {
    local path
    rm -rf -- "${sm_made[@]}"
    for path in "${sm_made_root[@]}"; do
        if [ -e "$path" ]; then sudo -n rm -rf -- "$path" 2>/dev/null || true; fi
    done
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

# Whether tools prompt, as with verbose output. Else a script spares each prompt
# with the tool's flag: `if asks; then yes=(); else yes=(--noconfirm); fi`.
asks() {
    [ "${SM_TERMINAL:-}" = on ]
}

# A closed terminal stops no run: sudo passes pacman no hangup. A run that
# elevates, named by $SM_RUN, holds its run_lock, shared so runs never wait for
# each other, and where logind answers shutdown and sleep off, each through a
# process that ends with the script.
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

# The lock a run of this script holds, for a query to wait on, in this user's
# runtime folder, which only this user writes. None without one, as for root
# through sudo.
run_lock() {
    local dir="${BASH_SOURCE[-1]%/*}"
    if ! [ -O "${XDG_RUNTIME_DIR:-}" ]; then return 1; fi
    echo "${XDG_RUNTIME_DIR}/system-manager-${dir##*/}.lock"
}

# Verbose output shows every program before it runs, as `$ sudo pacman -Syu`,
# through xtrace, which sm_trace turns on for a program alone. The space first,
# as bash repeats the first character of PS4 for a subshell.
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

# Names what a run found, for what may come after it.
found() {
    printf 'found\t%s\n' "$1" >&3
}

# Whether the package lists are newer than the system, as after an update that
# broke off: pacman -S would then mix versions. pacman ends with 1 when nothing
# is pending, and a package it ignores stays as it is.
behind() {
    local pending
    pending="$(pacman -Qu)" || allow 1
    pending="$(grep -v ' \[ignored\]$' <<<"$pending")" || allow 1
    [ -n "$pending" ]
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

# A folder of the operation's own as $work, made at its first use.
scratch() {
    if [ -n "${work:-}" ]; then return 0; fi
    work="$(mktemp -d)"
    sm_made+=("$work")
}

# The name a file is written under beside its target before it takes the
# target's place, as $new, which the caller declares local:
# `beside <file>; <command> >"$new"; mv -f -- "$new" <file>`. Absolute, as the
# script may change folders, with the links in it kept.
beside() {
    new="$(realpath -ms -- "$1").new"
    sm_made+=("$new")
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

# The helper on AUR packages. With its review, on unless $SM_AUR_REVIEW is off,
# and in verbose output, the person answers it on the terminal, which the
# runtime shows from the line `terminal` on fd 3. Else it goes without the
# review and the menus a run cannot answer.
aur_sync() {
    if asks || [ "${SM_AUR_REVIEW:-on}" != off ]; then
        printf 'terminal\n' >&3
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

# An extension of extensions.gnome.org through the running GNOME Shell, which
# asks the person, takes the version for itself and loads it at once. The call
# returns with the answer, or with an error. The shell itself serves it: the
# name org.gnome.Shell.Extensions starts a helper that drops a call left
# waiting for the person.
ego_install() {
    local answer
    answer="$(busctl --user --timeout=300s call org.gnome.Shell /org/gnome/Shell \
        org.gnome.Shell.Extensions InstallRemoteExtension s "$1")" || return 1
    if [ "$answer" != 's "successful"' ]; then return 1; fi
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
