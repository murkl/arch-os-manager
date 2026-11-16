# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

# Each argument is package=command: a command that answers works, a function of
# these scripts tells by itself. An argument @name asks for the fact of that
# name. All are asked at once, as some take a while to answer, and told in the
# order given. pacman names the versions in one call.
op_probe() {
    local pair name version line n=0
    local -a packages=()
    local -A versions=()
    scratch
    for pair in "$@"; do
        probe "$pair" >"${work}/${n}" 3>&- &
        n=$((n + 1))
        if [[ "$pair" != @* ]]; then packages+=("${pair%%=*}"); fi
    done
    if [ "${#packages[@]}" -gt 0 ]; then
        while read -r name version; do versions[$name]="$version"; done < <(pacman -Q -- "${packages[@]}" 2>/dev/null)
    fi
    wait
    for ((n = 0; n < $#; n++)); do
        line="$(<"${work}/${n}")"
        if [[ "$line" == *$'\tok\t+' ]]; then line="${line%+}${versions[${line%%$'\t'*}]-}"; fi
        printf '%s\n' "$line"
    done
}

# One of them: its line, and a `+` in place of the version of a command that
# answered.
probe() {
    local pair="$1" package argv
    if [[ "$pair" == @* ]]; then
        if "fact_${pair#@}"; then printf '%s\tok\n' "$pair"; else printf '%s\tmissing\n' "$pair"; fi
        return 0
    fi
    package="${pair%%=*}"
    read -ra argv <<<"${pair#*=}"
    if declare -F "${argv[0]}" >/dev/null; then
        if "${argv[@]}" >/dev/null; then printf '%s\tok\t\n' "$package"; else printf '%s\tmissing\n' "$package"; fi
    elif ! command -v "${argv[0]}" >/dev/null; then
        printf '%s\tmissing\n' "$package"
    elif timeout 10 "${argv[@]}" </dev/null >/dev/null 2>&1; then
        printf '%s\tok\t+\n' "$package"
    else
        printf '%s\tbroken\n' "$package"
    fi
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

# An optional package, set up as recommended. An AUR helper is built here from
# source, as a -bin build breaks at the next libalpm soname, and its build
# dependencies go again.
op_install() {
    local package="$1" conf report zone country yes
    if behind; then exit 11; fi
    case "$package" in
    paru | yay)
        sudo pacman -S --needed git base-devel
        scratch
        fetch "https://aur.archlinux.org/cgit/aur.git/snapshot/${package}.tar.gz" >"${work}/snapshot.tar.gz" || exit 10
        tar -xzf "${work}/snapshot.tar.gz" -C "$work"
        if asks; then yes=(); else yes=(--noconfirm); fi
        (cd "${work}/${package}" && makepkg --syncdeps --install --rmdeps "${yes[@]}")
        ;;
    *) sudo pacman -S --needed -- "$package" ;;
    esac

    # reflector's own configuration ranks by age, worldwide. As packaged, it gets
    # lines appended, which have the last word: by speed, within the country
    # zone.tab names for the time zone where Arch has a mirror there. One whose
    # content pacman finds changed stays. pacman ends with 1 on altered files of
    # the package itself.
    if [ "$package" = reflector ]; then
        conf=/etc/xdg/reflector/reflector.conf
        report="$(pacman -Qkk reflector 2>&1)" || allow 1
        if grep -qE "^backup file: reflector: ${conf//./\\.} \((Size|[A-Z0-9]* checksum) mismatch\)$" <<<"$report"; then
            printf 'kept\t%s\n' "$conf"
        else
            zone="$(readlink /etc/localtime)" || zone=""
            country="$(awk -v zone="${zone#*zoneinfo/}" '!/^#/ && $3 == zone { print $1 }' /usr/share/zoneinfo/zone.tab)"
            if [ -n "$country" ] && ! reflector --list-countries 2>/dev/null |
                awk -v code="$country" '$(NF - 1) == code { found = 1 } END { exit !found }'; then
                country=""
            fi
            {
                printf '%s\n' '' '# System Manager' '--age 12' '--latest 10' '--sort rate'
                if [ -n "$country" ]; then printf -- '--country %s\n' "$country"; fi
            } | sudo tee -a "$conf" >/dev/null
        fi
        sudo systemctl enable --now reflector.timer
    fi
    # Without its timer, fwupd knows only the firmware of its last look.
    if [ "$package" = fwupd ]; then sudo systemctl enable --now fwupd-refresh.timer; fi
}

# What a package started goes with it. An AUR helper may be its -bin or -git
# package, so the owner of its command decides.
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
