# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

# system or user: the loaded units with their state, then the unit files with theirs.
op_list() {
    local scope=() units files
    if [ "$1" = user ]; then scope=(--user); fi
    units="$(systemctl "${scope[@]}" list-units --type=service --all --output=json --no-pager)"
    files="$(systemctl "${scope[@]}" list-unit-files --type=service --output=json --no-pager)"
    printf 'units\t%s\nfiles\t%s\n' "$units" "$files"
}

# system or user, then the unit: its latest journal entries.
op_log() {
    if [ "$1" = user ]; then
        journalctl --user-unit "$2" -n 30 --output=json --no-pager
    else
        journalctl --unit "$2" -n 30 --output=json --no-pager
    fi
}

# start, stop, restart, enable or disable, then the unit.
op_system() {
    sudo systemctl "$1" -- "$2"
}

op_user() {
    systemctl --user "$1" -- "$2"
}

operation "$@"
