# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

# fwupdmgr ends with 2 when nothing is to update. Old metadata is refreshed by
# fwupd-refresh.timer, which installing fwupd here enables, and asking about it
# here would wait for an answer.
op_list() {
    fwupdmgr get-updates --json --no-metadata-check --no-remote-check || allow 2
}

# Yes to every question but uploading a report to the LVFS, which it never asks.
op_update() {
    local yes
    step refresh
    sudo fwupdmgr refresh --force || exit 10
    step update
    if asks; then yes=(); else yes=(--assume-yes); fi
    sudo fwupdmgr update "${yes[@]}" --no-reboot-check --no-unreported-check
}

operation "$@"
