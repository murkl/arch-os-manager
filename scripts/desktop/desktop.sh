# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

data="${XDG_DATA_HOME:-$HOME/.local/share}"
entry="${data}/applications/system-manager.desktop"
theme="${data}/icons/hicolor"
icon="${theme}/scalable/apps/system-manager.svg"
symbolic="${theme}/symbolic/apps/system-manager-symbolic.svg"
# Notifications show the Arch Linux mark of the package filesystem, the menu entry
# its own icon.
logo=/usr/share/pixmaps/archlinux-logo.svg
units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
service="${units}/system-manager-notify.service"
timer="${units}/system-manager-notify.timer"
wants="${units}/timers.target.wants/system-manager-notify.timer"

# Writes standard input to a file only where it differs, so a start that changes
# nothing touches nothing. Prints the file when it wrote.
put() {
    mkdir -p "${1%/*}"
    cat >"${1}.new"
    if cmp -s "${1}.new" "$1"; then
        rm -f "${1}.new"
    else
        mv -f "${1}.new" "$1"
        echo "$1"
    fi
}

# Whether this user's service manager answers: in the session it does, in the
# installer's chroot it does not, and the timer then starts with the next login.
manager() {
    systemctl --user show-environment >/dev/null 2>&1
}

# Whether the manager reads the timer from where it was written. A throwaway
# configuration, as make run sets, lies where it does not look.
known() {
    systemctl --user cat system-manager-notify.timer >/dev/null 2>&1
}

# Points the update indicator at the program and reads each command back, as
# GSettings prints a string: in double quotes where it holds a single one.
indicator() {
    local dir key value
    dir="$(arch_update_dir)"
    while IFS=$'\t' read -r key value; do
        if [[ $value == *\'* ]]; then
            value="\"${value//\"/\\\"}\""
        else
            value="'${value}'"
        fi
        on_bus gsettings --schemadir "${dir}/schemas" set org.gnome.shell.extensions.arch-update "$key" "$value"
        if [ "$(on_bus gsettings --schemadir "${dir}/schemas" get org.gnome.shell.extensions.arch-update "$key")" != "$value" ]; then
            exit 13
        fi
    done <<COMMANDS
check-cmd	"${1}" check
update-cmd	"${1}" --window upgrade
package-manager	"${1}" --window
COMMANDS
}

# The program, then on or off for the menu entry, how often the timer announces
# or off, and on or off for the update indicator, set up only where installed.
# All follow the program wherever it lies.
op_apply() {
    local program="$1" every changed=""
    case "$program" in
    *[\"\\\`\$%]*) exit 10 ;;
    esac
    case "$3" in
    hourly) every=1h ;;
    daily) every=1d ;;
    weekly) every=1w ;;
    off) every="" ;;
    *) false ;;
    esac

    if [ "$2" = on ]; then
        changed="$(put "$icon" <"${BASH_SOURCE[0]%/*}/icon.svg")"
        changed+="$(put "$symbolic" <"${BASH_SOURCE[0]%/*}/icon-symbolic.svg")"
        put "$entry" >/dev/null <<ENTRY
[Desktop Entry]
Type=Application
Name=System Manager
Icon=system-manager
Exec="${program}" --window
Terminal=false
Categories=System;
StartupWMClass=system-manager
ENTRY
    else
        if [ -e "$icon" ] || [ -e "$symbolic" ]; then changed=1; fi
        rm -f "$entry" "$icon" "$symbolic"
    fi
    # GTK trusts a cache another program left in the theme, with no entry for
    # this icon, until the theme is newer than the cache.
    if [ -n "$changed" ]; then touch "$theme"; fi

    if [ -n "$every" ]; then
        changed=""
        changed+="$(
            put "$service" <<UNIT
[Unit]
Description=Announce system updates

[Service]
Type=oneshot
ExecStart="${program}" notify --new
UNIT
        )"
        changed+="$(
            put "$timer" <<UNIT
[Unit]
Description=Announce system updates

[Timer]
OnStartupSec=2min
OnUnitActiveSec=${every}

[Install]
WantedBy=timers.target
UNIT
        )"
        # Enabled as systemctl enables it, which needs no running manager.
        mkdir -p "${wants%/*}"
        ln -sfn ../system-manager-notify.timer "$wants"
        if manager; then
            if [ -n "$changed" ]; then systemctl --user daemon-reload; fi
            if known; then systemctl --user start system-manager-notify.timer; fi
        fi
    elif [ -e "$timer" ]; then
        if manager && known; then systemctl --user stop system-manager-notify.timer; fi
        rm -f "$wants" "$timer" "$service"
        if manager; then systemctl --user daemon-reload; fi
    fi

    if [ "$4" = on ]; then indicator "$program"; fi
}

# The notification server as it runs now: the bus and the server's name on it.
# An id names a notification only for the server that gave it, which a new login
# or a restarted shell starts anew.
server() {
    local bus owner
    bus="$(busctl --user --timeout=5s call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus GetId)" || return 1
    owner="$(busctl --user --timeout=5s call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus \
        GetNameOwner s org.freedesktop.Notifications)" || return 1
    bus="${bus#s \"}" owner="${owner#s \"}"
    printf '%s %s\n' "${bus%\"}" "${owner%\"}"
}

# The id of the notification a handle names, where the server that gave it still
# runs.
standing() {
    local now
    if [ -z "$1" ]; then return 1; fi
    now="$(server)" || return 1
    if [ "${1% *}" != "$now" ]; then return 1; fi
    printf '%s\n' "${1##* }"
}

# Title, text, button label or nothing, seconds to wait, and the handle of the
# standing notification, which goes first. Prints this one's handle once shown,
# then the chosen action or `standing` on timeout, the header icon coming from
# the menu entry the hint names.
op_notify() {
    local server id="" action waiting status=0
    # Closed rather than replaced: GNOME tells only the program that showed a
    # notification that it closed. The specification answers the close of one
    # already gone with an error.
    op_close "${5:-}" || true
    server="$(server)" || server=""
    scratch
    mkfifo "${work}/id"
    timeout "$4" notify-send --app-name="System Manager" --icon="$logo" \
        --hint=string:desktop-entry:system-manager --wait ${3:+"--action=open=$3"} \
        --id-fd=4 -- "$1" "$2" 4>"${work}/id" >"${work}/action" &
    waiting=$!
    # The id comes once it shows, and the handle with it, while it still waits.
    read -r id <"${work}/id" || true
    if [[ $id =~ ^[0-9]+$ ]] && [ -n "$server" ]; then printf 'shown\t%s %s\n' "$server" "$id"; fi
    wait "$waiting" || status=$?
    case "$status" in
    0) while read -r action || [ -n "$action" ]; do printf 'chosen\t%s\n' "$action"; done <"${work}/action" ;;
    124) echo standing ;;
    *) return "$status" ;;
    esac
}

# Closes the notification a handle names, where its server still runs. A server that does
# not answer, as a frozen shell, holds the call for the limit and no longer.
op_close() {
    local id
    id="$(standing "$1")" || return 0
    busctl --user --timeout=5s call org.freedesktop.Notifications /org/freedesktop/Notifications \
        org.freedesktop.Notifications CloseNotification u "$id"
}

# Started as its own unit, so it outlives the timer's service that asks for it.
op_open() {
    systemd-run --user --collect --quiet -- "$@"
}

# The update indicator from extensions.gnome.org, through the running GNOME
# Shell, which asks the person, takes the version for itself and loads it at
# once. The call returns with the answer, or with an error. The shell itself
# serves it: the name org.gnome.Shell.Extensions starts a helper that drops a
# call left waiting for the person.
op_extension_install() {
    local answer
    answer="$(busctl --user --timeout=300s call org.gnome.Shell /org/gnome/Shell \
        org.gnome.Shell.Extensions InstallRemoteExtension s "$ARCH_UPDATE")" || exit 11
    if [ "$answer" != 's "successful"' ]; then exit 11; fi
}

# Takes the indicator out of this user's GNOME, with the commands it was given,
# through the running shell, which unloads it.
op_extension_remove() {
    local dir
    dir="$(arch_update_dir)" || return 0
    case "$dir" in /usr/*) exit 12 ;; esac
    gsettings --schemadir "${dir}/schemas" reset-recursively org.gnome.shell.extensions.arch-update
    gnome-extensions disable "$ARCH_UPDATE"
    gnome-extensions uninstall "$ARCH_UPDATE"
}

operation "$@"
