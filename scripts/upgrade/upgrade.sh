# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

# What an update would bring: repo, aur or flatpak, the name, the version now and the
# one to come. A source that cannot be asked is a line `failed` and its name, so the
# others still count.
op_list() {
    # checkupdates syncs a copy of the databases, so nothing half-synced is
    # left, and ends with 2 when nothing is pending. Two at once, as the timer
    # and the interface may run, share that copy.
    local status=0 repo pending apps
    repo="$(flock --wait 100 "${TMPDIR:-/tmp}/checkup-db-${UID}.lock" checkupdates)" || status=$?
    case "$status" in
    0) while read -r name old _ new; do printf 'repo\t%s\t%s\t%s\n' "$name" "$old" "$new"; done <<<"$repo" ;;
    2) ;;
    *) exit 10 ;;
    esac

    # The helper ends with 1 both when nothing is pending and when it failed,
    # and only a failure says why. A package pacman ignores stays as it is.
    if [ -n "$SM_AUR" ]; then
        scratch
        pending="$(aur -Qua 2>"${work}/errors")" || true
        if [ -s "${work}/errors" ] && [ -z "$pending" ]; then
            cat "${work}/errors" >&2
            printf 'failed\taur\n'
        fi
        while read -r name old _ new rest; do
            if [ -n "$name" ] && [[ $rest != *'[ignored]'* ]]; then
                printf 'aur\t%s\t%s\t%s\n' "$name" "$old" "$new"
            fi
        done <<<"$pending"
    fi

    # A version is often missing or unchanged, a commit never, and read collapses empty
    # fields between tabs, so the version comes last. A name pending in two branches,
    # as a runtime may be, carries its branch.
    if [ "$SM_FLATPAK" = on ]; then
        local installed app arch branch commit version ref old new name
        local -A versions commits pending_in
        if ! installed="$(flatpak list --columns=application,arch,branch,active,version)" ||
            ! apps="$(flatpak remote-ls --updates --columns=application,arch,branch,commit,version)"; then
            printf 'failed\tflatpak\n'
            return 0
        fi
        while IFS=$'\t' read -r app arch branch commit version; do
            versions["$app/$arch/$branch"]="$version"
            commits["$app/$arch/$branch"]="$commit"
        done <<<"$installed"
        while IFS=$'\t' read -r app _; do
            if [ -n "$app" ]; then pending_in["$app"]=$((${pending_in["$app"]:-0} + 1)); fi
        done <<<"$apps"
        while IFS=$'\t' read -r app arch branch commit version; do
            if [ -z "$app" ]; then continue; fi
            ref="$app/$arch/$branch"
            old="${versions[$ref]-}" new="$version"
            if [ -z "$old" ] || [ -z "$new" ] || [ "$old" = "$new" ]; then
                old="${commits[$ref]-}" new="$commit"
            fi
            name="$app"
            if [ "${pending_in[$app]}" -gt 1 ]; then name="${app}//${branch}"; fi
            printf 'flatpak\t%s\t%s\t%s\n' "$name" "$old" "$new"
        done <<<"$apps"
    fi
}

# Waits the seconds given at most while an upgrade of this user runs, as the
# update indicator checks a few seconds after pacman, while Flatpak may still
# update. flock ends with 1 when the time runs out.
op_idle() {
    local lock status=0
    lock="$(run_lock)" || return 0
    flock --wait "$1" "$lock" true || status=$?
    case "$status" in
    0) ;;
    1) exit 12 ;;
    *) false ;;
    esac
}

# Repositories first, then what is built on top of them. A failing AUR helper
# leaves Flatpak still to update.
op_run() {
    local aur=0 yes
    step repo
    sudo pacman -Syu
    if [ -n "$SM_AUR" ]; then
        step aur
        aur_sync -Sua || aur=$?
    fi
    if [ "$SM_FLATPAK" = on ]; then
        step flatpak
        if asks; then yes=(); else yes=(--assumeyes --noninteractive); fi
        flatpak update "${yes[@]}"
    fi
    if [ "$aur" -ne 0 ]; then exit 11; fi
}

operation "$@"
