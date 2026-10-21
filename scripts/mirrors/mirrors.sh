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

operation "$@"
