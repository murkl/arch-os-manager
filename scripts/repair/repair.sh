# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

op_run() {
    local lock holder
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

    # The keyring first, so the packages of the upgrade are checked against its keys.
    step keyring
    sudo pacman -Sy --needed archlinux-keyring
    step upgrade
    sudo pacman -Su
}

# The keyring anew from the keyring packages, built beside the old one, which
# stays until the new one is complete. The daemons gpg started for it end
# before it moves, else one answers for it from the old place.
op_keyring() {
    local gpg
    gpg="$(pacman-conf GPGDir)"
    gpg="${gpg%/}"
    case "$gpg" in
    */gnupg) ;;
    *) exit 11 ;;
    esac
    sudo rm -rf -- "${gpg}.new" "${gpg}.old"
    sudo pacman-key --gpgdir "${gpg}.new" --init
    sudo pacman-key --gpgdir "${gpg}.new" --populate
    sudo gpgconf --homedir "${gpg}.new" --kill all
    if [ -e "$gpg" ]; then sudo mv -- "$gpg" "${gpg}.old"; fi
    sudo mv -- "${gpg}.new" "$gpg"
    sudo rm -rf -- "${gpg}.old"
}

# Finds each package whose files lost content, type or existence as pacman -Qkk
# checks them, never for owner, mode or time, which a filesystem such as the
# vfat of /boot keeps its own way. Backup files of hand-edited configuration
# never count. A native package is found for a reinstall, an AUR package only
# named.
op_files() {
    local report altered native foreign name
    step check
    report="$(sudo pacman -Qkk 2>&1)" || true
    if ! grep -q ': [0-9]* total files\?, ' <<<"$report"; then
        printf '%s\n' "$report" >&2
        exit 12
    fi
    grep '^warning:' <<<"$report" || true
    altered="$(sed -n -e 's/^warning: \([^:]*\): .* (\(File type\|Size\|Symlink path\|[A-Z0-9]* checksum\) mismatch)$/\1/p' \
        -e 's/^warning: \([^:]*\): .* (No such file or directory)$/\1/p' <<<"$report" | sort -u)"
    if [ -z "$altered" ]; then return 0; fi
    mapfile -t native < <(comm -12 <(printf '%s\n' "$altered") <(pacman -Qqn | sort))
    mapfile -t foreign < <(comm -23 <(printf '%s\n' "$altered") <(pacman -Qqn | sort))
    if [ -n "${foreign[*]}" ]; then printf 'foreign, not reinstalled: %s\n' "${foreign[*]}"; fi
    for name in "${native[@]}"; do found "$name"; done
}

# The packages given, in the version installed, which the package lists hold
# only while they are not newer than the system.
op_reinstall() {
    if behind; then exit 13; fi
    sudo pacman -S -- "$@"
}

operation "$@"
