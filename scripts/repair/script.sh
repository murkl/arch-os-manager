# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

op_run() {
    local lock gpg holder
    # libalpm holds its lock open while it works, whichever front end runs it, so
    # only a lock nobody holds is a leftover. find ends with 1 when a process went
    # away during the walk, and what it printed still holds.
    lock="$(pacman-conf DBPath)db.lck"
    if [ -e "$lock" ]; then
        holder="$(sudo find /proc/[0-9]*/fd -maxdepth 1 -lname "$lock" -print -quit 2>/dev/null)" || true
        if [ -n "$holder" ]; then exit 10; fi
        step lock
        sudo rm -f -- "$lock"
    fi

    # Built beside the old keyring, which stays until the new one is complete.
    step keyring
    gpg="$(pacman-conf GPGDir)"
    gpg="${gpg%/}"
    case "$gpg" in
    */gnupg) ;;
    *) exit 11 ;;
    esac
    sudo rm -rf -- "${gpg}.new" "${gpg}.old"
    sudo pacman-key --gpgdir "${gpg}.new" --init
    sudo pacman-key --gpgdir "${gpg}.new" --populate
    if [ -e "$gpg" ]; then sudo mv -- "$gpg" "${gpg}.old"; fi
    sudo mv -- "${gpg}.new" "$gpg"
    sudo rm -rf -- "${gpg}.old"

    # The keyring first, so the packages of the upgrade are checked against the new keys.
    step upgrade
    sudo pacman -Sy --needed archlinux-keyring
    sudo pacman -Su
}

operation "$@"
