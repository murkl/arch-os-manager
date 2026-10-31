# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

list=/etc/pacman.d/mirrorlist

op_list() {
    local changed
    changed="$(stat -c %Y "$list")"
    printf 'changed\t%s\n' "$changed"
    if systemctl is-enabled --quiet reflector.timer 2>/dev/null; then printf 'timer\ton\n'; else printf 'timer\toff\n'; fi
    sed -n 's/^[[:space:]]*Server[[:space:]]*=[[:space:]]*/server\t/p' "$list"
}

# reflector.service reads /etc/xdg/reflector/reflector.conf, as installing
# reflector here writes it or as the person keeps it. A condition that is not
# met, such as being offline, skips it.
op_rank() {
    sudo systemctl start reflector.service || exit 11
    if [ "$(systemctl show --property=ConditionResult --value reflector.service)" = no ]; then exit 10; fi
}

# The editor of the person, on the terminal. sudoedit edits a copy and puts it
# back, so the editor itself runs unprivileged.
op_edit() {
    local status=0
    printf 'terminal\ton\n' >&3
    sudo -e -- "$list" || status=$?
    printf 'terminal\toff\n' >&3
    [ "$status" = 0 ] || exit 12
}

operation "$@"
