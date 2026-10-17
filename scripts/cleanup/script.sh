# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

# What the clean-up removes, one line each: orphan, cache, keep, aur, flatpak.
op_plan() {
    local orphans names protect=() dir status key caches list file size cache base
    # Packages installed as a dependency that nothing needs any more, with their size.
    orphans="$(pacman -Qdtq)" || [ $? -eq 1 ]
    if [ -n "$orphans" ]; then
        mapfile -t names <<<"$orphans"
        pacman -Qi -- "${names[@]}" | awk -F' *: ' '/^Name /{name=$2} /^Installed Size /{print "orphan\t" name "\t" $2}'
    fi

    # The Recovery rebuilds the boot files of a rolled back snapshot from the kernel
    # package that snapshot has installed, so those stay in the cache, whatever their age.
    if [ -d /.snapshots ]; then
        if [ ! -r /.snapshots ] || [ ! -x /.snapshots ]; then exit 10; fi
        for dir in /.snapshots/*/snapshot/var/lib/pacman/local/linux*/; do
            status=0
            grep -q -m 1 '^usr/lib/modules/[^/]*/vmlinuz$' "${dir}files" || status=$?
            case "$status" in
            0) protect+=("$(basename "$dir")") ;;
            1) ;;
            *) exit 10 ;;
            esac
        done
    fi
    mapfile -t protect < <(printf '%s\n' "${protect[@]}" | sort -u)
    for key in "${protect[@]}"; do
        if [ -n "$key" ]; then printf 'keep\t%s\n' "$key"; fi
    done

    # paccache's own choice, as arch-os trims by hand: three versions of what is
    # installed, none of what is not.
    mapfile -t caches < <(pacman-conf CacheDir)
    list="$(candidates --keep 3 && candidates --uninstalled --keep 0)"
    while read -r file; do
        if [ -z "$file" ]; then continue; fi
        for key in "${protect[@]}"; do
            if [ -n "$key" ] && [[ $file == "$key"-* ]]; then continue 2; fi
        done
        for cache in "${caches[@]}"; do
            if [ -f "${cache%/}/$file" ]; then
                size="$(stat -c %s "${cache%/}/$file")"
                printf 'cache\t%s\t%s\n' "${cache%/}/$file" "$size"
                break
            fi
        done
    done < <(sort -u <<<"$list")

    # Build directories of AUR helpers for packages no longer installed.
    local -A installed=()
    while read -r base; do installed[$base]=1; done < <(pacman -Qq)
    while read -r base; do installed[$base]=1; done < <(awk '/^%BASE%$/ { getline; print }' "$(pacman-conf DBPath)"local/*/desc)
    cache="${XDG_CACHE_HOME:-$HOME/.cache}"
    for dir in "$cache"/paru/clone/*/ "$cache"/yay/*/; do
        if [ -z "${installed[$(basename "$dir")]:-}" ]; then
            size="$(du -sb "$dir" | cut -f 1)"
            printf 'aur\t%s\t%s\n' "${dir%/}" "$size"
        fi
    done

    # flatpak lists unused runtimes only while removing them.
    if command -v flatpak >/dev/null; then printf 'flatpak\n'; fi
}

candidates() {
    paccache --dryrun --verbose --nocolor "$@" | sed -n '/^==> Candidate packages:/,/^$/{/^==>/d;/^$/d;p}'
}

# Found again right before removing, never taken from an earlier look.
op_run() {
    local plan caches files builds file dir inside cache orphans
    plan="$(op_plan)" || exit $?
    mapfile -t orphans < <(field orphan "$plan")
    if [ -n "${orphans[*]}" ]; then
        step orphans
        sudo pacman -Rns --noconfirm -- "${orphans[@]}"
    fi

    mapfile -t caches < <(pacman-conf CacheDir)
    mapfile -t files < <(field cache "$plan")
    if [ -n "${files[*]}" ]; then
        step cache
        for file in "${files[@]}"; do
            inside=""
            for dir in "${caches[@]}"; do
                if [[ $file == "${dir%/}"/*.pkg.tar.* ]]; then inside=1; fi
            done
            if [ -z "$inside" ]; then exit 11; fi
        done
        # The signature goes with its package, as paccache removes it.
        sudo rm -f -- "${files[@]}" "${files[@]/%/.sig}"
    fi

    cache="${XDG_CACHE_HOME:-$HOME/.cache}"
    mapfile -t builds < <(field aur "$plan")
    if [ -n "${builds[*]}" ]; then
        step aur
        for dir in "${builds[@]}"; do
            if [[ $dir != "$cache"/paru/clone/?* && $dir != "$cache"/yay/?* ]]; then exit 11; fi
        done
        rm -rf -- "${builds[@]}"
    fi

    if command -v flatpak >/dev/null; then
        step flatpak
        flatpak uninstall --unused --assumeyes --noninteractive
    fi
}

# The second column of the plan's lines of one kind.
field() {
    awk -F '\t' -v kind="$1" '$1 == kind { print $2 }' <<<"$2"
}

operation "$@"
