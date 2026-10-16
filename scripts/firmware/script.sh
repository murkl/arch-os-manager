# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

# fwupdmgr ends with 2 when nothing is to update. Old metadata is refreshed by
# its own timer, and asking about it here would wait for an answer.
op_list() {
    fwupdmgr get-updates --json --no-metadata-check --no-remote-check || [ $? -eq 2 ]
}

op_update() {
    step refresh
    sudo fwupdmgr refresh --force || exit 10
    step update
    sudo fwupdmgr update --assume-yes --no-reboot-check
}

operation "$@"
