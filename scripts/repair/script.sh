# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

op_run() {
    local lock gpg
    # A lock with pacman running belongs to it; only a leftover is removed.
    if pgrep -x pacman >/dev/null; then exit 10; fi
    lock="$(pacman-conf DBPath)db.lck"
    if [ -e "$lock" ]; then
        step lock
        sudo rm -f -- "$lock"
    fi

    step keyring
    gpg="$(pacman-conf GPGDir)"
    case "$gpg" in
    */gnupg | */gnupg/) ;;
    *) exit 11 ;;
    esac
    sudo rm -rf -- "${gpg:?}"
    sudo pacman-key --init
    sudo pacman-key --populate

    # The keyring first, so the packages of the upgrade are checked against the new keys.
    step upgrade
    sudo pacman -Sy --needed archlinux-keyring
    sudo pacman -Su
}

operation "$@"
