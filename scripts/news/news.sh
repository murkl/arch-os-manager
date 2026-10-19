# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

op_fetch() {
    fetch https://archlinux.org/feeds/news/ || exit 10
}

operation "$@"
