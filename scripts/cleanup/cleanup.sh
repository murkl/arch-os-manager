# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

# What the clean-up removes, one line each: orphan, cache, aur, then what each container
# engine there is says.
op_plan() {
    local orphans names dir caches list file size cache base
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

    docker_plan
    podman_plan
}

# The system's Docker, asked only while it runs: its socket would start it, and with it every
# container set to restart. The socket is where systemd listens, writable for the group docker.
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

# The parts to clean up, by id. What they remove is found again right before removing, never
# taken from an earlier look, in a shell of its own: a substitution tested by || would run the
# plan without errexit.
op_run() {
    local plan caches files builds file dir inside cache orphans socket part yes taken=" $* "
    for part; do
        case "$part" in orphans | cache | aur | flatpak | docker | podman) ;; *) false ;; esac
    done
    plan="$(bash "${BASH_SOURCE[0]}" plan)" || exit $?
    mapfile -t orphans < <(field orphan "$plan")
    if [[ $taken == *" orphans "* ]] && [ -n "${orphans[*]}" ]; then
        step orphans
        if asks; then yes=(); else yes=(--noconfirm); fi
        sudo pacman -Rns "${yes[@]}" -- "${orphans[@]}"
    fi

    mapfile -t caches < <(pacman-conf CacheDir)
    mapfile -t files < <(field cache "$plan")
    if [[ $taken == *" cache "* ]] && [ -n "${files[*]}" ]; then
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
    if [[ $taken == *" aur "* ]] && [ -n "${builds[*]}" ]; then
        step aur
        for dir in "${builds[@]}"; do
            if [[ $dir != "$cache"/paru/clone/?* && $dir != "$cache"/yay/?* ]]; then exit 11; fi
        done
        rm -rf -- "${builds[@]}"
    fi

    # Flatpak finds unused runtimes only while removing them.
    if [[ $taken == *" flatpak "* ]]; then
        step flatpak
        if asks; then yes=(); else yes=(--assumeyes --noninteractive); fi
        flatpak uninstall --unused "${yes[@]}"
    fi

    # Images first, so the build cache they shared goes free with it. Containers and volumes
    # hold data and stay.
    if asks; then yes=(); else yes=(--force); fi
    if [[ $taken == *" docker "* ]] && [ "$(field docker "$plan")" = ready ]; then
        step docker
        socket="$(systemctl show --property=Listen --value docker.socket)"
        socket="${socket%% *}"
        docker --host "unix://${socket}" image prune --all "${yes[@]}"
        docker --host "unix://${socket}" builder prune --all "${yes[@]}"
    fi
    if [[ $taken == *" podman "* ]] && [ "$(field podman "$plan")" = ready ]; then
        step podman
        podman image prune --all "${yes[@]}"
    fi
}

# The second column of the plan's lines of one kind.
field() {
    awk -F '\t' -v kind="$1" '$1 == kind { print $2 }' <<<"$2"
}

operation "$@"
