# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

# system or user: the loaded units with their state, then the unit files with theirs. The
# system's also names the units the graphical session cannot do without, the display manager
# and the bus, by what their alias stands for. GNOME's own user units refuse a manual stop.
op_list() {
    local scope=() units files session id
    if [ "$1" = user ]; then scope=(--user); fi
    units="$(systemctl "${scope[@]}" list-units --type=service --all --output=json --no-pager)"
    files="$(systemctl "${scope[@]}" list-unit-files --type=service --output=json --no-pager)"
    printf 'units\t%s\nfiles\t%s\n' "$units" "$files"
    if [ "$1" = system ]; then
        session="$(systemctl show --property=Id --value display-manager.service dbus.service)"
        while read -r id; do
            if [ -n "$id" ]; then printf 'session\t%s\n' "$id"; fi
        done <<<"$session"
    fi
}

# system or user, then the unit: its latest journal entries. Without --all a
# message over 4 KiB, such as a core dump's stack trace, comes as null.
op_log() {
    if [ "$1" = user ]; then
        journalctl --user-unit "$2" -n 30 --output=json --all --no-pager
    else
        journalctl --unit "$2" -n 30 --output=json --all --no-pager
    fi
}

# start, stop, restart, enable or disable, then the unit.
op_system() {
    verb "$1"
    sudo systemctl "$1" -- "$2"
}

op_user() {
    verb "$1"
    systemctl --user "$1" -- "$2"
}

# Only what the page offers reaches systemctl.
verb() {
    case "$1" in start | stop | restart | enable | disable) ;; *) false ;; esac
}

operation "$@"
