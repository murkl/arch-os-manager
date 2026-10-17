# shellcheck source=../lib.sh
. "${BASH_SOURCE[0]%/*}/../lib.sh" || exit 1

# What an update would bring: repo, aur or flatpak, the name, the version now and the one to come.
op_list() {
    # checkupdates syncs a copy of the databases, so nothing half-synced is left
    # behind. It ends with 2 when there is nothing to update.
    local status=0 repo pending apps
    repo="$(checkupdates)" || status=$?
    case "$status" in
    0) while read -r name old _ new; do printf 'repo\t%s\t%s\t%s\n' "$name" "$old" "$new"; done <<<"$repo" ;;
    2) ;;
    *) exit 10 ;;
    esac

    # The helper ends with 1 both when nothing is pending and when it failed; only
    # a failure says why.
    if [ -n "$SM_AUR" ]; then
        errors="$(mktemp)"
        trap 'rm -f "$errors"' EXIT
        pending="$(aur -Qua 2>"$errors")" || true
        if [ -s "$errors" ] && [ -z "$pending" ]; then
            cat "$errors" >&2
            exit 11
        fi
        while read -r name old _ new _; do
            if [ -n "$name" ]; then printf 'aur\t%s\t%s\t%s\n' "$name" "$old" "$new"; fi
        done <<<"$pending"
    fi

    # A version is often missing or unchanged, a commit never; read collapses empty
    # fields between tabs, so the version comes last.
    if command -v flatpak >/dev/null; then
        local installed app arch branch commit version ref old new
        local -A versions commits
        installed="$(flatpak list --columns=application,arch,branch,active,version)" || exit 12
        apps="$(flatpak remote-ls --updates --columns=application,arch,branch,commit,version)" || exit 12
        while IFS=$'\t' read -r app arch branch commit version; do
            versions["$app/$arch/$branch"]="$version"
            commits["$app/$arch/$branch"]="$commit"
        done <<<"$installed"
        while IFS=$'\t' read -r app arch branch commit version; do
            if [ -z "$app" ]; then continue; fi
            ref="$app/$arch/$branch"
            old="${versions[$ref]-}" new="$version"
            if [ -z "$old" ] || [ -z "$new" ] || [ "$old" = "$new" ]; then
                old="${commits[$ref]-}" new="$commit"
            fi
            printf 'flatpak\t%s\t%s\t%s\n' "$app" "$old" "$new"
        done <<<"$apps"
    fi
}

# Repositories first, then what is built on top of them.
op_run() {
    step repo
    sudo pacman -Syu
    if [ -n "$SM_AUR" ]; then
        step aur
        aur_sync -Sua
    fi
    if command -v flatpak >/dev/null; then
        step flatpak
        flatpak update --assumeyes --noninteractive
    fi
}

operation "$@"
