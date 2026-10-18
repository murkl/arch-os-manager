# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

op_state() {
    local hostname version
    hostname="$(</proc/sys/kernel/hostname)"
    printf 'hostname\t%s\n' "$hostname"
    printf 'kernel\t%s\n' "$(uname -r)"
    # The boot in seconds since the epoch, as the kernel counts it.
    awk '$1 == "btime" { printf "booted\t%s\n", $2 }' /proc/stat
    version="$(pacman -Q pacman)"
    printf 'pacman\t%s\n' "${version#* }"
    # The desktop of the session it runs in. GNOME also tells its version.
    if [ -n "${XDG_CURRENT_DESKTOP:-}" ]; then
        if [ "$XDG_CURRENT_DESKTOP" = GNOME ] && command -v gnome-shell >/dev/null; then
            version="$(gnome-shell --version)"
            printf 'desktop\tGNOME %s\n' "${version##* }"
        else
            printf 'desktop\t%s\n' "${XDG_CURRENT_DESKTOP//:/, }"
        fi
    fi
    if [ -n "${XDG_SESSION_TYPE:-}" ]; then printf 'session\t%s\n' "$XDG_SESSION_TYPE"; fi
    # An updated kernel removes the running one's modules: nothing new loads until a restart.
    if [ -d "/usr/lib/modules/$(uname -r)" ]; then
        printf 'reboot\tno\n'
    else
        printf 'reboot\tyes\n'
    fi
}

# Whether sudo goes without the password. Without a prompt PAM is not asked, so
# nothing counts towards the lock of pam_faillock, as a prompt left unanswered does.
op_nopasswd() {
    sudo --non-interactive --validate 2>/dev/null || exit 10
}

# Proves the password before a run needs it, started only with one to give.
op_auth() {
    sudo --validate
}

op_reboot() {
    systemctl reboot
}

operation "$@"
