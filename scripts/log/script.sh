# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

# The errors of this boot, newest last. Members of wheel read the system journal.
# Without --all a message over 4 KiB, such as a stack trace, comes as null.
op_errors() {
    journalctl --boot --priority=3 --lines=500 --output=json --all --no-pager
}

operation "$@"
