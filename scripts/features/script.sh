# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

# Each argument is package=command, the command with its arguments: a command
# that answers works, a function of these scripts tells by itself whether it is
# there. An argument @name asks for the fact of that name.
op_probe() {
    local pair package argv version
    for pair in "$@"; do
        if [[ "$pair" == @* ]]; then
            if "fact_${pair#@}"; then printf '%s\tok\n' "$pair"; else printf '%s\tmissing\n' "$pair"; fi
            continue
        fi
        package="${pair%%=*}"
        read -ra argv <<<"${pair#*=}"
        if declare -F "${argv[0]}" >/dev/null; then
            if "${argv[@]}" >/dev/null; then printf '%s\tok\t\n' "$package"; else printf '%s\tmissing\n' "$package"; fi
        elif ! command -v "${argv[0]}" >/dev/null; then
            printf '%s\tmissing\n' "$package"
        elif timeout 10 "${argv[@]}" </dev/null >/dev/null 2>&1; then
            version="$(pacman -Q "$package" 2>/dev/null)" || version=""
            printf '%s\tok\t%s\n' "$package" "${version#* }"
        else
            printf '%s\tbroken\n' "$package"
        fi
    done
}

# A desktop is installed: a display manager has a session to offer.
fact_desktop() {
    local sessions=(/usr/share/wayland-sessions/*.desktop /usr/share/xsessions/*.desktop)
    [ "${#sessions[@]}" -gt 0 ]
}

# The window speaks Wayland only.
fact_wayland() {
    local sessions=(/usr/share/wayland-sessions/*.desktop)
    [ "${#sessions[@]}" -gt 0 ]
}

fact_gnome() {
    command -v gnome-shell >/dev/null
}

# An optional package, set up as recommended. An AUR helper cannot come from itself:
# it is built here from source, as Arch OS does, since a -bin build breaks at the
# next libalpm soname. Its build dependencies go again.
op_install() {
    local package="$1" zone country
    case "$package" in
    paru | yay)
        sudo pacman -S --needed git base-devel
        scratch
        fetch "https://aur.archlinux.org/cgit/aur.git/snapshot/${package}.tar.gz" >"${work}/snapshot.tar.gz" || exit 10
        tar -xzf "${work}/snapshot.tar.gz" -C "$work"
        (cd "${work}/${package}" && makepkg --syncdeps --install --rmdeps --noconfirm)
        ;;
    *) sudo pacman -S --needed -- "$package" ;;
    esac

    # reflector's own configuration ranks by age, this one by speed, within the
    # country of the time zone, as Arch OS reads it from zone.tab, where Arch has
    # a mirror there, and weekly.
    if [ "$package" = reflector ]; then
        zone="$(readlink /etc/localtime)" || zone=""
        country="$(awk -v zone="${zone#*zoneinfo/}" '!/^#/ && $3 == zone { print $1 }' /usr/share/zoneinfo/zone.tab)"
        if [ -n "$country" ] && ! reflector --list-countries 2>/dev/null |
            awk -v code="$country" '$(NF - 1) == code { found = 1 } END { exit !found }'; then
            country=""
        fi
        {
            printf '%s\n' '--save /etc/pacman.d/mirrorlist' '--protocol https' '--age 12' '--latest 10' '--sort rate'
            if [ -n "$country" ]; then printf -- '--country %s\n' "$country"; fi
        } | sudo install -m 644 /dev/stdin /etc/xdg/reflector/reflector.conf
        sudo systemctl enable --now reflector.timer
    fi
    # Without its timer, fwupd knows only the firmware of its last look.
    if [ "$package" = fwupd ]; then sudo systemctl enable --now fwupd-refresh.timer; fi
}

# What a package started goes with it. An AUR helper may be its -bin or -git
# package. The owner of its command tells.
op_remove() {
    local package="$1"
    case "$package" in
    paru | yay) package="$(pacman -Qqo -- "$(command -v "$package")")" ;;
    reflector) sudo systemctl disable --now reflector.timer ;;
    fwupd) sudo systemctl disable --now fwupd-refresh.timer ;;
    esac
    sudo pacman -Rns -- "$package"
}

operation "$@"
