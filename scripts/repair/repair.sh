# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

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

# Every file of every package against what the package brought, as pacman -Qkk
# checks them. A configuration changed by hand is a backup file, which pacman
# lists apart and never counts. A package counts by what a reinstall restores:
# content, type or a missing file, never the owner, mode or time, which a
# filesystem such as the vfat of /boot keeps its own way. One with such a file
# comes back from its repository, one from the AUR has none and is only named.
op_files() {
    local report altered native foreign
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
    if [ -n "${native[*]}" ]; then
        step reinstall
        sudo pacman -S -- "${native[@]}"
    fi
}

operation "$@"
