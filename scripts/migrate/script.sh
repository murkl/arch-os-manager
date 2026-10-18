# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

# The Arch OS Manager 1.x, taken over once. When no 1.x is left to take over,
# remove this folder, the MIGRATE operations in src/script/mod.rs, and
# src/migrate.rs with its calls in src/cli.rs and src/ui/app.rs.

autostart="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/arch-os.desktop"

# Its own autostart entry, told by what it started.
ours() {
    [ -f "$autostart" ] && grep -qF 'arch-os notify' "$autostart"
}

# What the old manager left in its home, the first argument, and in the
# autostart, each as `left` and its path, then its settings that have a
# counterpart, as key and value.
op_read() {
    local home="$1" path key value aur_support="" aur_repo=""
    for path in "${home}/bin" "${home}/config/settings.conf" "${home}/config/blacklist.conf" \
        "${home}/config/kitty.conf" "${home}/database"; do
        if [ -e "$path" ]; then printf 'left\t%s\n' "$path"; fi
    done
    if ours; then printf 'left\t%s\n' "$autostart"; fi
    if [ ! -f "${home}/config/settings.conf" ]; then return 0; fi
    # A shell file the old manager sourced, read here as text, the value up to a
    # blank or a comment.
    while IFS='=' read -r key value; do
        value="${value%%[[:space:]#]*}"
        case "$key" in
        AUTOSTART_NOTIFY) if [ "$value" = false ]; then printf 'notify\tfalse\n'; fi ;;
        AUR_SUPPORT) aur_support="$value" ;;
        AUR_MANGER_REPO) aur_repo="$value" ;;
        esac
    done <"${home}/config/settings.conf"
    if [ "$aur_support" = false ]; then
        printf 'aur-helper\tnone\n'
    elif [[ "$aur_repo" == yay* ]]; then
        printf 'aur-helper\tyay\n'
    fi
}

# Removes what the old manager left in its home, the first argument, and puts
# the System Manager in its place in the GNOME dock.
op_clean() {
    local home="$1"
    rm -rf -- "${home:?}/bin" "${home:?}/database"
    rm -f -- "${home:?}/config/settings.conf" "${home:?}/config/blacklist.conf" "${home:?}/config/kitty.conf"
    if [ -d "${home}/config" ]; then rmdir --ignore-fail-on-non-empty -- "${home}/config"; fi
    if ours; then rm -f -- "$autostart"; fi
    dock
}

# The dock's entry of the old manager becomes the System Manager's, or goes
# where the System Manager is in the dock already.
dock() {
    local favorites
    if ! command -v gsettings >/dev/null; then return 0; fi
    favorites="$(on_bus gsettings get org.gnome.shell favorite-apps 2>/dev/null)" || return 0
    if [[ "$favorites" != *"'arch-os.desktop'"* ]]; then return 0; fi
    if [[ "$favorites" == *"'system-manager.desktop'"* ]]; then
        favorites="${favorites//"'arch-os.desktop', "/}"
        favorites="${favorites//", 'arch-os.desktop'"/}"
        favorites="${favorites//"'arch-os.desktop'"/}"
    else
        favorites="${favorites//"'arch-os.desktop'"/"'system-manager.desktop'"}"
    fi
    on_bus gsettings set org.gnome.shell favorite-apps "$favorites"
    if [[ "$(on_bus gsettings get org.gnome.shell favorite-apps)" == *"'arch-os.desktop'"* ]]; then exit 10; fi
}

operation "$@"
