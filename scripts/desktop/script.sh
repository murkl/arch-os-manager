# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

data="${XDG_DATA_HOME:-$HOME/.local/share}"
entry="${data}/applications/system-manager.desktop"
theme="${data}/icons/hicolor"
icon="${theme}/scalable/apps/system-manager.svg"
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

# The program, then on or off for the app menu entry, how often the timer
# announces updates or off, and on or off for the update indicator, which is
# set up only where it is installed. All follow the program wherever it lies.
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
        changed="$(put "$icon" <"${BASH_SOURCE[0]%/*}/logo.svg")"
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
        if [ -e "$icon" ]; then changed=1; fi
        rm -f "$entry" "$icon"
    fi
    # GTK trusts a cache another program left in the theme, with no entry for
    # this icon, until the theme is newer than the cache.
    if [ -n "$changed" ]; then touch "$theme"; fi

    if [ -n "$every" ]; then
        changed=""
        changed+="$(put "$service" <<UNIT
[Unit]
Description=Announce system updates

[Service]
Type=oneshot
ExecStart="${program}" notify --new
UNIT
)"
        changed+="$(put "$timer" <<UNIT
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

# Title, text, the label of the one action and the seconds to wait for it. Prints the
# action if it was chosen. Unanswered, it ends as if dismissed. GNOME takes the
# icon in the header from the menu entry the hint names.
op_notify() {
    timeout "$4" notify-send --app-name="System Manager" --icon=system-manager \
        --hint=string:desktop-entry:system-manager \
        --action="open=$3" -- "$1" "$2" || allow 124
}

# Started as its own unit, so it outlives the timer's service that asks for it.
op_open() {
    systemd-run --user --collect --quiet -- "$@"
}

# The update indicator from extensions.gnome.org, in the version for this GNOME
# Shell. GNOME loads an extension it did not start with at the next login.
op_extension_install() {
    local shell info url
    shell="$(gnome-shell --version)"
    shell="${shell##* }"
    info="$(fetch "https://extensions.gnome.org/extension-info/?uuid=${ARCH_UPDATE}&shell_version=${shell%%.*}")" || exit 11
    url="$(sed -n 's/.*"download_url": *"\([^"]*\)".*/\1/p' <<<"$info")"
    if [ -z "$url" ]; then exit 11; fi
    scratch
    fetch "https://extensions.gnome.org${url}" >"${work}/extension.zip" || exit 11
    gnome-extensions install --force "${work}/extension.zip"
    on_bus gnome-extensions enable "$ARCH_UPDATE"
}

# Takes the indicator out of this user's GNOME, with the commands it was given.
op_extension_remove() {
    local dir
    dir="$(arch_update_dir)" || return 0
    case "$dir" in /usr/*) exit 12 ;; esac
    on_bus gsettings --schemadir "${dir}/schemas" reset-recursively org.gnome.shell.extensions.arch-update
    on_bus gnome-extensions disable "$ARCH_UPDATE"
    # The running shell unloads it. Without one, the folder goes by hand.
    gnome-extensions uninstall "$ARCH_UPDATE" 2>/dev/null || rm -rf -- "$dir"
}

operation "$@"
