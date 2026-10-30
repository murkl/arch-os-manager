# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

# What the clean-up removes, one line each: orphan, cache, aur, then what each container
# engine there is says.
op_plan() {
    local orphans names dir caches list file size cache
    # Packages installed as a dependency that nothing needs any more, with their size.
    orphans="$(pacman -Qdtq)" || allow 1
    if [ -n "$orphans" ]; then
        mapfile -t names <<<"$orphans"
        pacman -Qi -- "${names[@]}" | awk -F' *: ' '/^Name /{name=$2} /^Installed Size /{print "orphan\t" name "\t" $2}'
    fi

    # paccache's own choice: three versions of what is installed, none of what
    # is not. It lists each signature too, which goes with its package.
    mapfile -t caches < <(pacman-conf CacheDir)
    list="$(candidates --keep 3 && candidates --uninstalled --keep 0)"
    while read -r file; do
        if [ -z "$file" ] || [[ $file == *.sig ]]; then continue; fi
        for cache in "${caches[@]}"; do
            if [ -f "${cache%/}/$file" ]; then
                size="$(stat -c %s "${cache%/}/$file")"
                printf 'cache\t%s\t%s\n' "${cache%/}/$file" "$size"
                break
            fi
        done
    done < <(sort -u <<<"$list")

    list="$(builds)"
    while read -r dir; do
        if [ -z "$dir" ]; then continue; fi
        size="$(du -sb "$dir" | cut -f 1)"
        printf 'aur\t%s\t%s\n' "$dir" "$size"
    done <<<"$list"

    docker_plan
    podman_plan
}

# The system's Docker, asked and cleaned up only while it runs: its socket would start it, and
# with it every container set to restart. The socket is where systemd listens, writable for the
# group docker.
docker_plan() {
    local socket usage images
    if ! docker --version >/dev/null 2>&1 || ! systemctl cat docker.service >/dev/null 2>&1; then
        return 0
    fi
    if ! systemctl is-active --quiet docker.service; then
        printf 'docker\tstopped\n'
        return 0
    fi
    socket="$(systemctl show --property=Listen --value docker.socket)"
    socket="${socket%% *}"
    if [ ! -w "$socket" ]; then
        printf 'docker\tdenied\n'
        return 0
    fi
    if ! usage="$(docker --host "unix://${socket}" system df --format json)" ||
        ! images="$(docker --host "unix://${socket}" system df --verbose --format json)"; then
        printf 'docker\tfailed\n'
        return 0
    fi
    printf 'docker\tready\n'
    tagged docker-df <<<"$usage"
    tagged docker-du <<<"$images"
}

# This user's own Podman storage, which podman reads without a daemon.
podman_plan() {
    local usage images
    if ! podman --version >/dev/null 2>&1; then return 0; fi
    if ! usage="$(podman system df --format '{{json .}}')" ||
        ! images="$(podman images --filter containers=false --format '{{.Repository}}:{{.Tag}}{{"\t"}}{{.ID}}')"; then
        printf 'podman\tfailed\n'
        return 0
    fi
    printf 'podman\tready\n'
    tagged podman-df <<<"$usage"
    if [ -n "$images" ]; then tagged podman-image <<<"$images"; fi
}

# Each line of standard input after a kind and a tab, as the plan's own lines are.
tagged() {
    local line
    while IFS= read -r line; do printf '%s\t%s\n' "$1" "$line"; done
}

candidates() {
    paccache --dryrun --verbose --nocolor "$@" | sed -n '/^==> Candidate packages:/,/^$/{/^==>/d;/^$/d;p}'
}

# Build directories of AUR helpers for packages no longer installed, one per line.
builds() {
    local base dir db cache="${XDG_CACHE_HOME:-$HOME/.cache}"
    local -A installed=()
    while read -r base; do installed[$base]=1; done < <(pacman -Qq)
    # pacman-conf prints DBPath as written, with a slash at its end or without.
    db="$(pacman-conf DBPath)"
    while read -r base; do installed[$base]=1; done < <(awk '/^%BASE%$/ { getline; print }' "${db%/}"/local/*/desc </dev/null)
    for dir in "$cache"/paru/clone/*/ "$cache"/yay/*/; do
        if [ -z "${installed[$(basename "$dir")]:-}" ]; then printf '%s\n' "${dir%/}"; fi
    done
}

# The parts to clean up, by id. Each finds what it removes right before removing, with the tool
# that removes it.
op_run() {
    local part found names yes socket
    local taken=" $* "
    for part; do
        case "$part" in orphans | cache | aur | flatpak | docker | podman) ;; *) false ;; esac
    done
    if [[ $taken == *" orphans "* ]]; then
        found="$(pacman -Qdtq)" || allow 1
        if [ -n "$found" ]; then
            step orphans
            mapfile -t names <<<"$found"
            if asks; then yes=(); else yes=(--noconfirm); fi
            sudo pacman -Rns "${yes[@]}" -- "${names[@]}"
        fi
    fi

    # The plan's choice, each signature with its package.
    if [[ $taken == *" cache "* ]]; then
        step cache
        sudo paccache --remove --keep 3
        sudo paccache --remove --uninstalled --keep 0
    fi

    if [[ $taken == *" aur "* ]]; then
        found="$(builds)"
        if [ -n "$found" ]; then
            step aur
            mapfile -t names <<<"$found"
            rm -rf -- "${names[@]}"
        fi
    fi

    # Flatpak finds unused runtimes only while removing them.
    if [[ $taken == *" flatpak "* ]]; then
        step flatpak
        if asks; then yes=(); else yes=(--assumeyes --noninteractive); fi
        flatpak uninstall --unused "${yes[@]}"
    fi

    # Images first, so the build cache they shared goes free with it. Containers and volumes
    # hold data and stay. As root, so the group docker is not needed.
    if asks; then yes=(); else yes=(--force); fi
    if [[ $taken == *" docker "* ]] && systemctl is-active --quiet docker.service; then
        step docker
        socket="$(systemctl show --property=Listen --value docker.socket)"
        socket="${socket%% *}"
        sudo docker --host "unix://${socket}" image prune --all "${yes[@]}"
        sudo docker --host "unix://${socket}" builder prune --all "${yes[@]}"
    fi
    if [[ $taken == *" podman "* ]] && podman --version >/dev/null 2>&1; then
        step podman
        podman image prune --all "${yes[@]}"
    fi
}

operation "$@"
