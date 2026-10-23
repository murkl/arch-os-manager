# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

# The homelab beside the profiles: homelab.yaml names the servers, each a folder with a folder per
# stack. A secret lies beside what it belongs to as <file>.age, under a key of the homelab's own.
lab=homelab
# The dotfiles, and the profile of this machine in them: the one chosen, else the host's name.
# The scripts of a profile get both.
export SM_DOTFILES="${HOME}/.dotfiles"
export SM_PROFILE="${SM_PROFILE:-$(</proc/sys/kernel/hostname)}"
# The profile as a path inside the dotfiles, where every operation works.
profile="$SM_PROFILE"
# What this machine keeps of the dotfiles and never commits: the age key once unlocked and what it
# applied, inside the git folder as git-crypt keeps its key. Set by enter.
mine=''
# The dotfiles with every link resolved, as a link into them reads. Set by enter.
here=''
# Where this run puts what stands in the way, made with the first.
backup=''
# The folder of an operation of its own, which scratch in lib.sh makes.
work=''
# What this machine applied, by path in the dotfiles: the blob and the time.
declare -A blob=() when=()
gnome=/org/gnome/shell/extensions/

# A query has no terminal to ask on, and no window may open for one: git and ssh fail instead. It
# reads beside a run and takes no lock it can do without, as git says of a status in the background.
if [ -z "${SM_RUN:-}" ]; then
    export GIT_TERMINAL_PROMPT=0 GIT_ASKPASS='' SSH_ASKPASS_REQUIRE=never GIT_OPTIONAL_LOCKS=0
fi
# Plain http and git:// are refused, also as the target of a redirect.
export GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=protocol.http.allow GIT_CONFIG_VALUE_0=never \
    GIT_CONFIG_KEY_1=protocol.git.allow GIT_CONFIG_VALUE_1=never

# A path of $HOME as the profile keeps it, .foo as dot-foo in every part, and back, as stow
# --dotfiles reads it.
encode() {
    local IFS=/ parts
    read -ra parts <<<"$1"
    printf '%s\n' "${parts[*]/#./dot-}"
}

decode() {
    local IFS=/ parts
    read -ra parts <<<"$1"
    printf '%s\n' "${parts[*]/#dot-/.}"
}

# Whether the dotfiles are a work tree of their own, not a bare repository or a folder in another.
repository() {
    local top
    top="$(git -C "$SM_DOTFILES" rev-parse --show-toplevel 2>/dev/null)" || return 1
    [ "$top" = "$(realpath -- "$SM_DOTFILES")" ]
}

# Into the dotfiles, with what this machine applied of them.
enter() {
    local path hash time
    if ! repository; then exit 10; fi
    cd -- "$SM_DOTFILES"
    here="$(pwd -P)"
    mine="$(git rev-parse --absolute-git-dir)/system-manager"
    if [ -f "${mine}/applied" ]; then
        while IFS=$'\t' read -r path hash time; do
            blob["$path"]="$hash"
            when["$path"]="$time"
        done <"${mine}/applied"
    fi
}

# Notes each path as applied in the state it has now, or forgets one that is gone, and writes the
# record anew.
remember() {
    local path
    for path; do
        if [ -f "$path" ]; then
            blob["$path"]="$(git hash-object -- "$path")"
            when["$path"]="$EPOCHSECONDS"
        else
            unset 'blob[$path]' 'when[$path]'
        fi
    done
    record
}

# Notes a function of dotfiles.sh as run here in the state its hash names.
ran() {
    local key="dotfiles.sh/$1"
    blob["$key"]="$2"
    when["$key"]="$EPOCHSECONDS"
    record
}

# The hash a function of dotfiles.sh last ran with here, empty where it never did.
applied() {
    local key="dotfiles.sh/$1"
    printf '%s\n' "${blob[$key]:-}"
}

# What this machine applied, written anew.
record() {
    local path new
    mkdir -p -- "$mine"
    beside "${mine}/applied"
    for path in "${!blob[@]}"; do
        printf '%s\t%s\t%s\n' "$path" "${blob[$path]}" "${when[$path]}"
    done | LC_ALL=C sort >"$new"
    mv -f -- "$new" "${mine}/applied"
}

# Whether a file of the dotfiles changed since this machine applied it, or never did.
changed() {
    [ "${blob[$1]:-}" != "$(git hash-object -- "$1")" ]
}

# A list of the profile without comments and blank lines, nothing where there is none.
listed() {
    if [ -f "$1" ]; then grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' -- "$1" || allow 1; fi
}

# Staged as it is added, as a global ignore file naming it, say CLAUDE.md, would keep it out of
# every backup.
track() {
    git add --force -- "$1"
}

# What a command prints, into a file only once it succeeded, and no file where it prints nothing:
# `keep <file> <command>`.
keep() {
    local out new
    out="$("${@:2}")"
    if [ -z "$out" ]; then
        rm -f -- "$1"
        return 0
    fi
    mkdir -p -- "$(dirname -- "$1")"
    beside "$1"
    printf '%s\n' "$out" >"$new"
    mv -f -- "$new" "$1"
}

# The repository of the dotfiles, made where there is none.
ensure_repository() {
    if [ ! -e "$SM_DOTFILES" ]; then
        git -c advice.defaultBranchName=false init --quiet -- "$SM_DOTFILES"
    fi
}

# The profile, made where it is missing as something goes into it, with its init.sh, and with
# dotfiles.yaml and dotfiles.sh beside it where the dotfiles lack them. They are the person's from
# then on: one deleted stays deleted. The template's init.sh does nothing, so it counts as run.
ensure_profile() {
    local template="${BASH_SOURCE[0]%/*}/template" file
    if [ -d "$profile" ]; then return 0; fi
    for file in dotfiles.yaml dotfiles.sh; do
        if [ ! -e "$file" ]; then cp -- "${template}/${file}" .; fi
    done
    mkdir -p -- "$profile"
    cp -- "${template}/profile/init.sh" "${profile}/"
    remember "${profile}/init.sh"
}

# The default branch of origin as last fetched, empty for a remote without one yet.
default_branch() {
    local ref
    ref="$(git symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null)" || return 0
    printf '%s\n' "${ref#origin/}"
}

# What the checked-out branch is measured against, as remote, branch and whether that is its
# upstream, one per line: its upstream, before the first push the default branch of origin.
target() {
    local branch remote merge
    branch="$(git branch --show-current)"
    if [ -n "$branch" ] && remote="$(git config "branch.${branch}.remote")" &&
        merge="$(git config "branch.${branch}.merge")" && [ "$remote" != . ]; then
        printf '%s\n' "$remote" "${merge#refs/heads/}" 1
    else
        printf '%s\n' origin "$(default_branch)" 0
    fi
}

# Exits with why git could not reach the repository, in git's own words: credentials, a branch
# gone, anything else.
unreached() {
    printf '%s\n' "$1" >&2
    if grep -qiE 'terminal prompts disabled|could not read (username|password)|authentication failed|permission denied|host key verification failed|no supported authentication methods|invalid username or (password|token)' <<<"$1"; then
        exit 11
    fi
    if grep -qiF "couldn't find remote ref" <<<"$1"; then exit 14; fi
    exit 12
}

# Fetches from origin, a branch of it where one is named.
fetch_remote() {
    local said
    said="$(git fetch --quiet origin "$@" 2>&1)" || unreached "$said"
}

# The checked-out branch onto what the remote holds for it: a fast-forward where only the remote
# moved on, a rebase where both did, so several machines keep one straight line. Open changes are
# stashed and come back. A rebase that needs a decision is taken back whole.
integrate() {
    local t ref counts
    mapfile -t t < <(target)
    # An empty remote has nothing to bring. The first push fills it.
    if [ -z "${t[1]}" ]; then return 0; fi
    fetch_remote "${t[1]}"
    ref="${t[0]}/${t[1]}"
    # Nothing committed here yet: the branch of the remote, as a clone takes it.
    if ! git rev-parse --verify -q HEAD >/dev/null; then
        git switch --quiet --create "${t[1]}" --track "$ref" || exit 36
        return 0
    fi
    if [ -z "$(git branch --show-current)" ]; then exit 13; fi
    if ! git merge-base HEAD "$ref" >/dev/null; then exit 36; fi
    counts="$(git rev-list --count --left-right "HEAD...${ref}")"
    if [ "${counts##*$'\t'}" -eq 0 ]; then return 0; fi
    if [ "${counts%%$'\t'*}" -eq 0 ]; then
        git merge --quiet --ff-only --autostash "$ref"
        return 0
    fi
    if ! git rebase --quiet --autostash "$ref"; then
        git rebase --abort || true
        exit 15
    fi
}

# Uploads the branch, bound to origin at its first push.
push() {
    local t said
    mapfile -t t < <(target)
    if [ "${t[2]}" = 1 ]; then
        said="$(git push --quiet 2>&1)" || unreached "$said"
    else
        said="$(git push --quiet --set-upstream origin "$(git branch --show-current)" 2>&1)" || unreached "$said"
    fi
}

# Whether origin is set.
remote() {
    git remote get-url origin >/dev/null 2>&1
}

# Moves what stands where the dotfiles put something into this run's backup, under its place in
# $HOME, and says so.
aside() {
    if [ -z "$backup" ]; then backup="${HOME}/.dotfiles-backup/$(date +%Y-%m-%d-%H%M%S)"; fi
    mkdir -p -- "$(dirname -- "${backup}/$1")"
    mv -- "${HOME}/$1" "${backup}/$1"
    echo "${HOME}/$1 -> ${backup}/$1"
}

# The home/ folders of this machine that exist: the profile's, then the one of every machine
# beside the profiles. A place in both is the profile's.
homes() {
    local dir
    for dir in "${profile}/home" home; do
        if [ -d "$dir" ]; then printf '%s\n' "$dir"; fi
    done
}

# Every file of home/, by its place in $HOME and its path in the dotfiles after a tab.
leaves() {
    local dir leaf place
    local -A seen=()
    while IFS= read -r -u 4 dir; do
        while IFS= read -r -d '' leaf; do
            place="$(decode "${leaf#"$dir"/}")"
            if [ -n "${seen[$place]:-}" ]; then continue; fi
            seen["$place"]=1
            printf '%s\t%s\n' "$place" "$leaf"
        done < <(find "$dir" -mindepth 1 \( -type f -o -type l \) -print0)
    done 4< <(homes)
}

# Every place of home/ in $HOME, a line each, as the record of the links keeps them.
places() {
    leaves | cut -f1
}

# Whether a place in $HOME is the link to a file of the dotfiles.
linked() {
    [ -L "${HOME}/$1" ] && [ "$(realpath -m -- "${HOME}/$1")" = "${here}/$2" ]
}

# Whether a place in $HOME is a link into a home/ of this machine.
ours() {
    local target
    if [ ! -L "${HOME}/$1" ]; then return 1; fi
    target="$(realpath -m -- "${HOME}/$1")"
    [[ $target == "${here}/home/"* || $target == "${here}/${profile}/home/"* ]]
}

# Takes the links to files that left home/ since the last link here: each place the record of it
# names that is a link into a home/ with nothing behind it.
prune() {
    local place
    if [ ! -f "${mine}/${profile}.links" ]; then return 0; fi
    while IFS= read -r place; do
        if ours "$place" && [ ! -e "${HOME}/${place}" ]; then rm -- "${HOME}/${place}"; fi
    done <"${mine}/${profile}.links"
}

# home/ into $HOME through stow, folders real, files linked. What stands where a link goes, a file
# of its own or a link elsewhere, goes into the backup first. A link of the other home/ makes way
# for the profile's and stays where the profile has nothing.
link() {
    local dir entry place
    prune
    while IFS= read -r -u 4 dir; do
        while IFS= read -r -d '' entry; do
            place="$(decode "${entry#"$dir"/}")"
            if [ -d "$entry" ] && [ ! -L "$entry" ]; then
                if [ -L "${HOME}/${place}" ] || { [ -e "${HOME}/${place}" ] && [ ! -d "${HOME}/${place}" ]; }; then
                    aside "$place"
                fi
            elif { [ -e "${HOME}/${place}" ] || [ -L "${HOME}/${place}" ]; } && ! linked "$place" "$entry"; then
                if ! ours "$place"; then
                    aside "$place"
                elif [ "$dir" = "${profile}/home" ]; then
                    rm -- "${HOME}/${place}"
                fi
            fi
        done < <(find "$dir" -mindepth 1 -print0)
    done 4< <(homes)
    stow_home || exit 16
    keep "${mine}/${profile}.links" places
}

# The profile's home/ first, then the one of every machine, which leaves the places the profile
# took.
stow_home() {
    if [ -d "${profile}/home" ]; then
        stow --dir="${here}/${profile}" --target="$HOME" --dotfiles --no-folding --restow home || return 1
    fi
    if [ -d home ]; then
        stow --dir="$here" --target="$HOME" --dotfiles --no-folding --restow --defer='.*' home || return 1
    fi
}

# The age key on this machine of the secrets of a folder of the dotfiles, a profile or the homelab.
keyfile() {
    printf '%s/%s.key\n' "$mine" "$1"
}

# Whether a folder holds secrets.
sealed() {
    [ -n "$(find "$1" -type f -name '*.age' ! -path "$1/key.age" -print -quit 2>/dev/null)" ]
}

# The key of a folder unlocked on this machine: out of its key.age with the passphrase, which age
# asks for, up to three times.
unlock() {
    local key new said
    key="$(keyfile "$1")"
    if [ -f "$key" ]; then return 0; fi
    if [ ! -f "$1/key.age" ]; then exit 23; fi
    mkdir -p -- "$mine"
    beside "$key"
    for _ in 1 2 3; do
        if said="$( (umask 077 && age --decrypt --output "$new" "$1/key.age") 2>&1)"; then
            mv -f -- "$new" "$key"
            return 0
        fi
        printf '%s\n' "$said" >&2
        if ! grep -q 'incorrect passphrase' <<<"$said"; then exit 1; fi
    done
    exit 20
}

# The key of a folder behind a passphrase into its key.age, which age asks for twice: the one of
# this machine, or a new one for a folder without secrets.
wrap() {
    local key new said
    key="$(keyfile "$1")"
    if [ ! -f "$key" ]; then
        if sealed "$1"; then exit 23; fi
        mkdir -p -- "$mine"
        beside "$key"
        (umask 077 && age-keygen --output "$new" 2>/dev/null)
        mv -f -- "$new" "$key"
    fi
    beside "$1/key.age"
    if ! said="$(age --passphrase --output "$new" "$key" 2>&1)"; then
        printf '%s\n' "$said" >&2
        if grep -q "didn't match" <<<"$said"; then exit 21; fi
        exit 1
    fi
    mv -f -- "$new" "$1/key.age"
}

# Every secret of the profile, by its place in $HOME and its path in the dotfiles after a tab.
secrets() {
    local file place
    if [ ! -d "${profile}/secrets" ]; then return 0; fi
    while IFS= read -r -d '' file; do
        place="$(decode "${file#"$profile"/secrets/}")"
        printf '%s\t%s\n' "${place%.age}" "$file"
    done < <(find "${profile}/secrets" -type f -name '*.age' -print0)
}

# Each secret into $HOME, readable by its owner only, where it is missing or its file in the
# profile changed since it was written here. What stands there otherwise goes into the backup.
restore_secrets() {
    local place file target key new
    unlock "$profile"
    key="$(keyfile "$profile")"
    while IFS=$'\t' read -r -u 4 place file; do
        target="${HOME}/${place}"
        if [ -f "$target" ] && [ ! -L "$target" ] && ! changed "$file"; then continue; fi
        mkdir -p -- "$(dirname -- "$target")"
        beside "$target"
        (umask 077 && age --decrypt --identity "$key" --output "$new" "$file")
        if [ -f "$target" ] && [ ! -L "$target" ] && cmp -s -- "$new" "$target"; then
            rm -- "$new"
        else
            if [ -e "$target" ] || [ -L "$target" ]; then aside "$place"; fi
            mv -- "$new" "$target"
        fi
        remember "$file"
    done 4< <(secrets)
}

# Whether a secret in $HOME differs from the profile's.
differs() {
    ! age --decrypt --identity "$(keyfile "$profile")" "$2" | cmp -s -- - "${HOME}/$1"
}

# Each secret whose file in $HOME differs from the profile's, encrypted anew into the profile.
save_secrets() {
    local place file recipient new
    recipient="$(age-keygen -y "$(keyfile "$profile")")"
    while IFS=$'\t' read -r -u 4 place file; do
        if [ -f "${HOME}/${place}" ] && differs "$place" "$file"; then
            beside "$file"
            age --encrypt --recipient "$recipient" --output "$new" "${HOME}/${place}"
            mv -f -- "$new" "$file"
            remember "$file"
        fi
    done 4< <(secrets)
}

# Whether GNOME Shell runs for this user, asked of the bus: gnome-extensions would start its
# service where none runs.
gnome_shell() {
    [ "$(busctl --user --timeout=5s call org.freedesktop.DBus /org/freedesktop/DBus \
        org.freedesktop.DBus NameHasOwner s org.gnome.Shell 2>/dev/null || true)" = 'b true' ]
}

# The active extensions and the settings of each into a folder, as the profile keeps them. An
# extension with a settings path of its own lies outside and is left out.
extensions() {
    local entry
    mkdir -p -- "$1"
    gnome-extensions list --enabled | LC_ALL=C sort >"$1/extensions.txt"
    while IFS= read -r entry; do
        case "$entry" in
        */) dconf dump "${gnome}${entry}" >"$1/${entry%/}.conf" ;;
        esac
    done < <(dconf list "$gnome")
}

# GSettings reads a list as a GVariant array of strings.
gvariant() {
    local items
    if [ $# -eq 0 ]; then
        printf '[]'
        return 0
    fi
    items="$(printf "'%s', " "$@")"
    printf '[%s]' "${items%, }"
}

# The packages installed on purpose, by origin. Flatpaks only from Flathub: an app from a bundle
# has no remote to come back from.
installed() {
    case "$1" in
    pacman) pacman -Qqen ;;
    aur) pacman -Qqem || allow 1 ;;
    flatpak)
        if command -v flatpak >/dev/null; then
            flatpak list --app --columns=application,origin | awk -F '\t' '$2 == "flathub" { print $1 }'
        fi
        ;;
    esac | LC_ALL=C sort -u
}

# Lines of a list missing from the output of a command: `lacking <list> <command>`.
lacking() {
    LC_ALL=C comm -23 <(listed "$1" | LC_ALL=C sort -u) <("${@:2}" | LC_ALL=C sort -u)
}

# A command in a shell of its own in a folder of the dotfiles, strict as makepkg runs a PKGBUILD,
# with dotfiles.sh read first where there is one: `strict <folder> <command>`. What dotfiles.sh
# prints as it is read goes to the details.
strict() {
    (cd -- "$1" && bash -c 'set -euo pipefail; if [ -f "$1" ]; then . "$1" >&2; fi; "${@:2}"' bash "${here}/dotfiles.sh" "${@:2}")
}

# The hash of a function of dotfiles.sh as bash reads it, so a comment or a space changes nothing.
# Empty where dotfiles.sh lacks it.
fingerprint() {
    local text
    text="$(strict . declare -f "$1" 2>/dev/null)" || return 0
    git hash-object --stdin <<<"$text"
}

# Whether every function named ran here once.
known() {
    local name
    for name; do
        if [ -z "$(applied "$name")" ]; then return 1; fi
    done
}

# Runs a function of dotfiles.sh and records it where it succeeds: `hook <function> <hash>`. 75,
# EX_TEMPFAIL of sysexits.h, says not yet: it runs again next time.
hook() {
    local status=0
    strict . "$1" || status=$?
    case "$status" in
    0) ran "$1" "$2" ;;
    75)
        echo "Not yet: $1"
        later=1
        ;;
    *) return 1 ;;
    esac
}

# A group's function as dotfiles.yaml names it after install: a name bash takes for a function.
group() {
    if [[ ! $1 =~ ^[A-Za-z_][A-Za-z0-9_-]*$ ]]; then exit 38; fi
}

# groups.txt with a function more.
with() {
    { listed "${profile}/groups.txt" && printf '%s\n' "$1"; } | LC_ALL=C sort -u
}

# groups.txt without a function.
without() {
    listed "${profile}/groups.txt" | grep -vxF -- "$1" || allow 1
}

# What a setup would apply and a backup would save, a line each. Nothing past the repository
# where it or the profile is missing.
op_state() {
    local t counts place file files=0 missing=0 name status
    local -a on=()
    printf 'dir\t%s\nprofile\t%s\nhost\t%s\n' "$SM_DOTFILES" "$SM_PROFILE" "$(</proc/sys/kernel/hostname)"
    if [ ! -e "$SM_DOTFILES" ]; then
        printf 'repo\tmissing\n'
        return 0
    fi
    if ! repository; then
        printf 'repo\tinvalid\n'
        return 0
    fi
    enter
    printf 'repo\tready\n'
    if remote; then printf 'remote\t%s\n' "$(git remote get-url origin)"; fi
    if git rev-parse --verify -q HEAD >/dev/null; then
        mapfile -t t < <(target)
        if [ -n "${t[1]}" ] && git rev-parse --verify -q "refs/remotes/${t[0]}/${t[1]}" >/dev/null; then
            counts="$(git rev-list --count --left-right "HEAD...${t[0]}/${t[1]}")"
            printf 'ahead\t%s\nbehind\t%s\n' "${counts%%$'\t'*}" "${counts##*$'\t'}"
        elif remote; then
            printf 'ahead\t%s\n' "$(git rev-list --count HEAD)"
        fi
    fi
    printf 'changes\t%s\n' "$(git status --porcelain | wc -l)"
    if [ -f "${lab}/homelab.yaml" ]; then printf 'homelab\tready\n'; fi
    if [ ! -d "$profile" ]; then
        printf 'new\tprofile\n'
        return 0
    fi
    while IFS=$'\t' read -r place file; do
        files=$((files + 1))
        if ! linked "$place" "$file"; then missing=$((missing + 1)); fi
    done < <(leaves)
    printf 'files\t%s\nlinks\t%s\n' "$files" "$missing"
    if [ -f "${mine}/${profile}.setup" ]; then printf 'setup\t%s\n' "$(<"${mine}/${profile}.setup")"; fi
    file="${profile}/init.sh"
    if [ -f "$file" ]; then
        status=applied
        if changed "$file"; then status=changed; fi
        printf 'init\t%s\t%s\n' "$status" "${when[$file]:-}"
    fi
    # A group switched on is due where it never ran here. Whether its function changed shows only
    # as dotfiles.sh is read, which a run does, so dotfiles.sh is due where it changed since.
    mapfile -t on < <(listed "${profile}/groups.txt")
    for name in "${on[@]}"; do
        status=applied
        if ! known "$name"; then status=changed; fi
        printf 'group\t%s\t%s\n' "$name" "$status"
    done
    if [ ${#on[@]} -gt 0 ] && [ -f dotfiles.sh ]; then
        status=applied
        if changed dotfiles.sh; then status=changed; fi
        printf 'functions\t%s\n' "$status"
    fi
    secrets_state
    gnome_state
    packages_state
    for file in "${HOME}"/.dotfiles-backup/*/; do printf 'backup\t%s\n' "${file%/}"; done
}

# The secrets: how many, whether the key is unlocked here, how many a setup would write and
# whether a backup would encrypt any anew.
secrets_state() {
    local place file total=0 due=0 key
    key="$(keyfile "$profile")"
    while IFS=$'\t' read -r place file; do
        total=$((total + 1))
        if [ ! -f "${HOME}/${place}" ] || changed "$file"; then due=$((due + 1)); fi
    done < <(secrets)
    printf 'secrets\t%s\n' "$total"
    if [ ! -f "${profile}/key.age" ]; then return 0; fi
    if [ ! -f "$key" ]; then
        printf 'key\tlocked\n'
    else
        printf 'key\tunlocked\n'
    fi
    if [ "$due" -gt 0 ]; then printf 'pending\tsecrets\t%s\n' "$due"; fi
    if [ -f "$key" ] && command -v age >/dev/null; then
        while IFS=$'\t' read -r place file; do
            if [ -f "${HOME}/${place}" ] && differs "$place" "$file"; then
                printf 'unsaved\tsecrets\n'
                return 0
            fi
        done < <(secrets)
    fi
}

# GNOME: whether the profile brings settings or a list this machine has not loaded, and whether
# this machine's differ from the profile's.
gnome_state() {
    local file
    if ! gnome_shell; then return 0; fi
    for file in "${profile}"/gnome/*; do
        if changed "$file"; then
            printf 'pending\tgnome\n'
            break
        fi
    done
    scratch
    extensions "${work}/gnome"
    if ! diff -rq -- "${work}/gnome" "${profile}/gnome" >/dev/null 2>&1; then printf 'unsaved\tgnome\n'; fi
}

# Whether the lists of the profile differ from what is installed here.
packages_state() {
    local list
    for list in pacman aur flatpak; do
        if ! cmp -s <(listed "${profile}/packages/${list}.txt" | LC_ALL=C sort -u) <(installed "$list"); then
            printf 'unsaved\tpackages\n'
            return 0
        fi
    done
}

# Fetched at the start, so ahead and behind tell what the remote holds now.
op_fetch() {
    if ! repository; then return 0; fi
    enter
    if remote; then fetch_remote; fi
}

# Clones the repository into the dotfiles, or binds the dotfiles to it. Empty, the dotfiles stay on
# this machine only.
op_connect() {
    local url="${1:-}" before said
    case "$url" in
    http://* | git://*) exit 37 ;;
    esac
    if [ ! -e "$SM_DOTFILES" ]; then
        if [ -z "$url" ]; then return 0; fi
        said="$(git clone --quiet -- "$url" "$SM_DOTFILES" 2>&1)" || unreached "$said"
        return 0
    fi
    enter
    before="$(git remote get-url origin 2>/dev/null || true)"
    if [ -z "$url" ]; then
        if [ -n "$before" ]; then git remote remove origin; fi
        return 0
    fi
    fits "$url"
    if [ -n "$before" ]; then git remote set-url origin "$url"; else git remote add origin "$url"; fi
    fetch_remote
    git remote set-head origin --auto >/dev/null 2>&1 || true
}

# Whether a repository answers and shares the history of the dotfiles, asked by its address alone,
# so nothing of origin changes before it fits. An empty one fits.
fits() {
    local said head
    said="$(git ls-remote --symref -- "$1" HEAD 2>&1)" || unreached "$said"
    head="$(sed -n 's#^ref: refs/heads/\(.*\)\tHEAD$#\1#p' <<<"$said")"
    # A HEAD without a commit, as an empty repository has, names no hash.
    if [ -z "$head" ] || ! grep -q $'^[0-9a-f][0-9a-f]*\tHEAD$' <<<"$said" ||
        ! git rev-parse --verify -q HEAD >/dev/null; then
        return 0
    fi
    said="$(git fetch --quiet -- "$1" "$head" 2>&1)" || unreached "$said"
    if ! git merge-base HEAD FETCH_HEAD >/dev/null; then exit 36; fi
}

# The machine as the profile describes it: the latest state, the links, the secrets, init, the
# packages, the groups and GNOME. What the profile brought since the last time is applied, what
# this machine changed since stays for a backup.
op_setup() {
    local name file uuid present lack repos status later=0
    local -a on=() wanted=() repo=() gone=() missing=() apps=()
    local -A hash=()
    enter
    if remote; then
        step pull
        integrate
    fi
    step link
    link
    if [ -n "$(secrets)" ]; then
        if ! command -v age >/dev/null; then exit 22; fi
        step secrets
        restore_secrets
    fi

    # The functions of the groups switched on where dotfiles.sh changed since its last setup here
    # or one of them never ran here. dotfiles.sh is read on its own first, so a failure of its own
    # is no missing function.
    mapfile -t on < <(listed "${profile}/groups.txt")
    for name in "${on[@]}"; do group "$name"; done
    if [ ${#on[@]} -gt 0 ] && { ! known "${on[@]}" || changed dotfiles.sh; }; then
        if [ ! -f dotfiles.sh ]; then exit 38; fi
        strict . true || exit 45
        for name in "${on[@]}"; do hash["$name"]="$(fingerprint "$name")"; done
    fi
    # init.sh where it changed since it last ran here, remembered as it is where it is done.
    if [ -f "${profile}/init.sh" ] && changed "${profile}/init.sh"; then
        step init
        status=0
        strict "$profile" . ./init.sh || status=$?
        case "$status" in
        0) remember "${profile}/init.sh" ;;
        75) echo "Not yet: init.sh" ;;
        *) exit 17 ;;
        esac
    fi

    step packages
    lack="$(lacking "${profile}/packages/pacman.txt" pacman -Qq)"
    if [ -n "$lack" ]; then
        # What no repository has any more is left out, as the Arch Wiki filters a list.
        repos="$(pacman -Slq | LC_ALL=C sort -u)"
        mapfile -t repo < <(LC_ALL=C comm -12 <(printf '%s\n' "$repos") <(printf '%s\n' "$lack"))
        mapfile -t gone < <(LC_ALL=C comm -13 <(printf '%s\n' "$repos") <(printf '%s\n' "$lack"))
        if [ ${#gone[@]} -gt 0 ]; then echo "In no repository: ${gone[*]}"; fi
        if [ ${#repo[@]} -gt 0 ]; then sudo pacman -S --needed -- "${repo[@]}"; fi
    fi
    mapfile -t missing < <(lacking "${profile}/packages/aur.txt" pacman -Qq)
    if [ ${#missing[@]} -gt 0 ]; then
        if [ -n "${SM_AUR:-}" ]; then
            aur_sync -S --needed -- "${missing[@]}"
        else
            echo "No AUR helper for: ${missing[*]}"
        fi
    fi
    mapfile -t apps < <(lacking "${profile}/packages/flatpak.txt" installed flatpak)
    if [ ${#apps[@]} -gt 0 ]; then
        if ! command -v flatpak >/dev/null; then
            echo "No Flatpak for: ${apps[*]}"
        elif asks; then
            flatpak install flathub "${apps[@]}"
        else
            flatpak install --noninteractive --assumeyes flathub "${apps[@]}"
        fi
    fi

    for name in "${on[@]}"; do
        if [ -z "${hash[$name]+set}" ] || [ "$(applied "$name")" = "${hash[$name]}" ]; then continue; fi
        if [ -z "${hash[$name]}" ]; then exit 38; fi
        step groups
        echo "$name"
        hook "$name" "${hash[$name]}" || exit 18
    done
    if [ ${#hash[@]} -gt 0 ] && [ "$later" = 0 ]; then remember dotfiles.sh; fi

    if [ -d "${profile}/gnome" ] && gnome_shell; then
        step gnome
        mapfile -t wanted < <(listed "${profile}/gnome/extensions.txt")
        present="$(gnome-extensions list)"
        for uuid in "${wanted[@]}"; do
            if ! grep -qxF -- "$uuid" <<<"$present"; then
                ego_install "$uuid" || echo "Not on extensions.gnome.org for this GNOME Shell: ${uuid}"
            fi
        done
        for file in "${profile}"/gnome/*.conf; do
            if ! changed "$file"; then continue; fi
            name="${file##*/}"
            dconf reset -f "${gnome}${name%.conf}/"
            dconf load "${gnome}${name%.conf}/" <"$file"
            remember "$file"
        done
        if [ -f "${profile}/gnome/extensions.txt" ] && changed "${profile}/gnome/extensions.txt"; then
            gsettings set org.gnome.shell enabled-extensions "$(gvariant "${wanted[@]}")"
            remember "${profile}/gnome/extensions.txt"
        fi
    fi
    keep "${mine}/${profile}.setup" printf '%s\n' "$EPOCHSECONDS"
}

# The parts of the dotfiles that changed, for the commit message: `nvim, packages`. A file of
# a home/ counts by its program, the folder below .config or .local/share or its own name.
areas() {
    git status --porcelain --untracked-files=all | cut -c4- | awk -F/ -v p="$profile" '
        { n = 0 }
        $1 == "home" { n = 2 }
        $1 == p && $2 == "home" { n = 3 }
        n && NF >= n {
            path = $n
            for (i = n + 1; i <= NF; i++) path = path "/" $i
            sub(/^dot-config\//, "", path)
            sub(/^dot-local\/share\//, "", path)
            sub(/^dot-local\//, "", path)
            split(path, part, "/")
            sub(/^dot-/, "", part[1])
            print part[1]
            next
        }
        $1 == p && NF >= 2 { sub(/\.txt$/, "", $2); print $2; next }
        { print $1 }
    ' | LC_ALL=C sort -u | paste -sd, - | sed 's/,/, /g'
}

# This machine into its profile: the packages, GNOME and the secrets that changed, then everything
# open committed and uploaded.
op_backup() {
    local list file new
    local -a stale=()
    ensure_repository
    enter
    ensure_profile
    step packages
    for list in pacman aur flatpak; do keep "${profile}/packages/${list}.txt" installed "$list"; done
    if gnome_shell; then
        step gnome
        # Written beside and swapped whole, so no file of an extension without settings stays.
        beside "${profile}/gnome"
        extensions "$new"
        rm -rf -- "${profile}/gnome"
        mv -- "$new" "${profile}/gnome"
        for file in "${!blob[@]}"; do
            if [[ $file == "${profile}/gnome/"* ]]; then stale+=("$file"); fi
        done
        remember "${stale[@]}" "${profile}"/gnome/*
    fi
    if [ -n "$(secrets)" ] && [ -f "$(keyfile "$profile")" ] && command -v age >/dev/null; then
        step secrets
        save_secrets
    fi
    if [ -n "$(git status --porcelain)" ]; then
        git var GIT_AUTHOR_IDENT >/dev/null 2>&1 || exit 19
        step commit
        git add -A
        git commit --quiet -m "Back up ${profile}: $(areas)"
    fi
    if remote && git rev-parse --verify -q HEAD >/dev/null; then
        step pull
        integrate
        step push
        push
    fi
}

# Runs the function of a group and switches it on in the profile, so a setup runs it again where
# it changed.
op_group_on() {
    local hash name later=0
    enter
    group "$1"
    if [ ! -f dotfiles.sh ]; then exit 38; fi
    strict . true || exit 45
    hash="$(fingerprint "$1")"
    if [ -z "$hash" ]; then exit 38; fi
    hook "$1" "$hash" || exit 18
    keep "${profile}/groups.txt" with "$1"
    # dotfiles.sh is applied here once every group on ran with its function as it is now.
    while IFS= read -r name; do
        if [ "$(applied "$name")" != "$(fingerprint "$name")" ]; then return 0; fi
    done < <(listed "${profile}/groups.txt")
    remember dotfiles.sh
}

# Switches a group off in the profile. What its function installed stays.
op_group_off() {
    enter
    keep "${profile}/groups.txt" without "$1"
}

# An XDG folder of this user, where its files are no configuration.
xdg() {
    case "${!1:-}" in
    /*) printf '%s\n' "${!1}" ;;
    *) printf '%s\n' "${HOME}/$2" ;;
    esac
}

# Exits with the reason a path of $HOME cannot go into the dotfiles, if there is one. A folder
# goes with at most 100 files: more is a program's data, not its configuration.
refuse() {
    local path="$1" skip files part
    local -a parts
    if [[ $path != "$HOME"/?* ]]; then exit 24; fi
    for skip in "$SM_DOTFILES" "$(xdg XDG_CACHE_HOME .cache)" "$(xdg XDG_STATE_HOME .local/state)"; do
        if [[ $path == "$skip" || $path == "$skip"/* ]]; then exit 25; fi
        if [[ $skip == "$path"/* ]]; then exit 26; fi
    done
    IFS=/ read -ra parts <<<"${path#"$HOME"/}"
    for part in "${parts[@]}"; do
        if [[ $part == dot-* ]]; then exit 35; fi
    done
    if [ -L "$path" ]; then exit 27; fi
    if [ -f "$path" ]; then return 0; fi
    if [ ! -d "$path" ]; then exit 28; fi
    if [ -n "$(find "$path" -name .git -print -quit)" ]; then exit 29; fi
    if [ -n "$(find "$path" ! -type d ! -type f -print -quit)" ]; then exit 30; fi
    files="$(find "$path" -type f -printf . | wc -c)"
    if [ "$files" -eq 0 ]; then exit 31; fi
    if [ "$files" -gt 100 ]; then exit 32; fi
}

# Whether the dotfiles keep a place already, or something in it, as a file or as a secret.
kept() {
    local place file
    while IFS=$'\t' read -r place file; do
        if [[ ${HOME}/$place == "$1" || ${HOME}/$place == "$1"/* ]]; then return 0; fi
    done < <(if [ -d "$SM_DOTFILES" ]; then cd -- "$SM_DOTFILES" && leaves && secrets; fi)
    return 1
}

# Moves a file or folder of $HOME into a home/ and links it back: `add shared|profile <path>`,
# the one of every machine or the profile's.
op_add() {
    local scope="$1" path="$2" dest
    case "$scope" in
    shared) dest=home ;;
    profile) dest="${profile}/home" ;;
    *) exit 46 ;;
    esac
    if kept "$path"; then exit 33; fi
    refuse "$path"
    ensure_repository
    enter
    ensure_profile
    dest="${dest}/$(encode "${path#"$HOME"/}")"
    mkdir -p -- "$(dirname -- "$dest")"
    mv -- "$path" "$dest"
    # Taken back whole where stow finds something else in the way.
    if ! stow_home; then
        mv -- "$dest" "$path"
        exit 16
    fi
    track "$dest"
    keep "${mine}/${profile}.links" places
}

# The links in $HOME to files of home/, as `file<TAB>path`, each folder holding nothing else as
# `dir<TAB>path/`, then each secret as `secret<TAB>path`.
op_managed() {
    local place file dir entry only
    local -a whole=()
    # A folder above a link is 1, one holding nothing but such links 2.
    local -A links=() folders=()
    enter
    while IFS=$'\t' read -r place file; do
        if ! linked "$place" "$file"; then continue; fi
        links["${HOME}/${place}"]=1
        dir="${HOME}/${place}"
        dir="${dir%/*}"
        while [[ $dir == "$HOME"/?* ]]; do
            folders["$dir"]=1
            dir="${dir%/*}"
        done
    done < <(leaves)
    # Deepest first, each folder read one level down: its subfolders are judged by then.
    while IFS= read -r dir; do
        only=1
        while IFS= read -r -d '' entry; do
            if [ -d "$entry" ] && [ ! -L "$entry" ]; then
                if [ "${folders[$entry]:-}" != 2 ]; then only=0; fi
            elif [ -z "${links[$entry]:-}" ]; then
                only=0
            fi
        done < <(find "$dir" -mindepth 1 -maxdepth 1 -print0)
        if [ "$only" = 1 ]; then
            folders["$dir"]=2
            whole+=("$dir")
        fi
    done < <(for dir in "${!folders[@]}"; do printf '%s\t%s\n' "${dir//[^\/]/}" "$dir"; done | sort -r | cut -f2-)
    {
        for place in "${!links[@]}"; do printf 'file\t%s\n' "$place"; done
        for dir in "${whole[@]}"; do printf 'dir\t%s/\n' "$dir"; done
    } | LC_ALL=C sort -t $'\t' -k2
    while IFS=$'\t' read -r place file; do printf 'secret\t%s\n' "${HOME}/${place}"; done < <(secrets) |
        LC_ALL=C sort -t $'\t' -k2
}

# Removes the folders left empty in the profile above a file, up to the folder given.
tidy() {
    local dir="${1%/*}"
    while [[ $dir == "$2"/* ]] && rmdir -- "$dir" 2>/dev/null; do
        dir="${dir%/*}"
    done
}

# Gives a link back as the file it points to, or every link of a folder, and takes the file out of
# its home/.
op_remove() {
    local chosen="${1%/}" place file new base found=0
    enter
    while IFS=$'\t' read -r -u 4 place file; do
        if [[ ${HOME}/$place != "$chosen" && ${HOME}/$place != "$chosen"/* ]] || ! linked "$place" "$file"; then
            continue
        fi
        beside "${HOME}/${place}"
        cp --preserve=mode,timestamps -- "$file" "$new"
        mv -f -- "$new" "${HOME}/${place}"
        rm -- "$file"
        base="${file%%/*}"
        if [ "$base" != home ]; then base="${base}/home"; fi
        tidy "$file" "$base"
        found=1
    done 4< <(leaves)
    if [ "$found" = 0 ]; then exit 34; fi
    keep "${mine}/${profile}.links" places
}

# Encrypts a file or every file of a folder of $HOME into secrets/. The file stays where it is.
op_secret_add() {
    local path="$1" file dest recipient new
    local -a files=()
    if kept "$path"; then exit 33; fi
    refuse "$path"
    ensure_repository
    enter
    ensure_profile
    if [ -f "${profile}/key.age" ]; then unlock "$profile"; else wrap "$profile"; fi
    recipient="$(age-keygen -y "$(keyfile "$profile")")"
    mapfile -d '' files < <(find "$path" -type f -print0)
    for file in "${files[@]}"; do
        dest="${profile}/secrets/$(encode "${file#"$HOME"/}").age"
        mkdir -p -- "$(dirname -- "$dest")"
        beside "$dest"
        age --encrypt --recipient "$recipient" --output "$new" "$file"
        mv -f -- "$new" "$dest"
        track "$dest"
        remember "$dest"
    done
}

# Takes a secret, or every secret of a folder, out of the profile. Its file in $HOME stays.
op_secret_remove() {
    local chosen="${1%/}" place file found=0
    enter
    while IFS=$'\t' read -r -u 4 place file; do
        if [[ ${HOME}/$place != "$chosen" && ${HOME}/$place != "$chosen"/* ]]; then continue; fi
        rm -- "$file"
        remember "$file"
        tidy "$file" "${profile}/secrets"
        found=1
    done 4< <(secrets)
    if [ "$found" = 0 ]; then exit 34; fi
}

# A new passphrase on the key of the secrets, which age asks for twice. Other machines unlocked
# already keep their key, the others unlock with the new passphrase.
op_passphrase() {
    enter
    if [ ! -f "${profile}/key.age" ]; then exit 39; fi
    unlock "$profile"
    wrap "$profile"
}

# A server of the homelab, as homelab.yaml names it: a host name with a folder beside it.
server() {
    if [[ ! $1 =~ ^[A-Za-z0-9][A-Za-z0-9.-]*$ ]]; then exit 40; fi
    if [ ! -d "${lab}/$1" ]; then exit 41; fi
}

# The homelab for a server: homelab.yaml naming it with the address of its Cup where there is none
# yet, and its stack cup, which tells what images have updates.
op_homelab_init() {
    local server="$1"
    if [[ ! $server =~ ^[A-Za-z0-9][A-Za-z0-9.-]*$ ]]; then exit 40; fi
    ensure_repository
    enter
    mkdir -p -- "${lab}/${server}"
    if [ ! -e "${lab}/${server}/cup" ]; then
        cp -r -- "${BASH_SOURCE[0]%/*}/template/homelab/cup" "${lab}/${server}/"
    fi
    if [ ! -e "${lab}/homelab.yaml" ]; then
        printf '%s\n' \
            '# The servers of the homelab, each a folder beside this file with a folder' \
            '# per stack. cup is where the Cup of the server answers, as its stack cup' \
            '# publishes it.' \
            'servers:' \
            "  - name: ${server}" \
            "    cup: http://${server}:8000" >"${lab}/homelab.yaml"
    fi
}

# What the Cup of a server knows of its images, as JSON.
op_homelab_cup() {
    fetch --max-time 5 "${1%/}/api/v3/json"
}

# What a tree holds, by type, mode, path and link target.
listing() {
    (cd -- "$1" && find . -mindepth 1 -printf '%y %m %P %l\n' | LC_ALL=C sort)
}

# A server of the homelab for its own tools: its folder with each secret decrypted in place of its
# .age, as <server>.tar in the downloads folder. Packed without anyone's name, unpacked again and
# compared before it takes its place, and readable by its owner only, as it holds the secrets.
op_homelab_export() {
    local server="$1" dir out stage file
    enter
    server "$server"
    if [ -n "$(find "${lab}/${server}" ! -type d ! -type f -print -quit)" ]; then exit 42; fi
    if sealed "${lab}/${server}"; then unlock "$lab"; fi
    dir="$(xdg-user-dir DOWNLOAD 2>/dev/null || true)"
    if [ ! -d "$dir" ]; then dir="$HOME"; fi
    # Beside the archive rather than in /tmp: renamed there, it is never half written.
    out="$(mktemp -d "${dir}/.system-manager.XXXXXX")"
    sm_made+=("$out")
    stage="${out}/stage"
    cp -r --preserve=timestamps -- "${lab}/${server}" "$stage"
    chmod -R u+rwX,go+rX,go-w "$stage"
    while IFS= read -r -d '' file; do
        (umask 077 && age --decrypt --identity "$(keyfile "$lab")" --output "${file%.age}" "$file")
        rm -- "$file"
    done < <(find "$stage" -type f -name '*.age' -print0)
    find "$stage" -name .gitkeep -delete
    (cd -- "$stage" && find . -mindepth 1 -printf '%P\n' | LC_ALL=C sort) |
        tar --create --file "${out}/archive" --directory "$stage" --no-recursion \
            --verbatim-files-from --files-from - --format=ustar --owner=0 --group=0 --numeric-owner
    mkdir -- "${out}/check"
    tar --extract --preserve-permissions --file "${out}/archive" --directory "${out}/check"
    if ! diff -r --no-dereference -- "$stage" "${out}/check" >&2 ||
        ! diff <(listing "$stage") <(listing "${out}/check") >&2; then
        exit 43
    fi
    chmod 600 -- "${out}/archive"
    mv -f -- "${out}/archive" "${dir}/${server}.tar"
    echo "${dir}/${server}.tar"
}

# A path in the folder of a server.
within() {
    case "$1" in
    /* | *..* | '' | */) exit 44 ;;
    esac
}

# A file of the home folder as a secret of a server, encrypted to <path>.age in its folder. One
# there before is replaced.
op_homelab_secret_add() {
    local server="$1" file="$2" path="$3" dest new
    enter
    server "$server"
    within "$path"
    if [ ! -f "$file" ]; then exit 28; fi
    if [ -f "${lab}/key.age" ]; then unlock "$lab"; else wrap "$lab"; fi
    dest="${lab}/${server}/${path}.age"
    mkdir -p -- "$(dirname -- "$dest")"
    beside "$dest"
    age --encrypt --recipient "$(age-keygen -y "$(keyfile "$lab")")" --output "$new" "$file"
    mv -f -- "$new" "$dest"
    track "$dest"
}

# Takes a secret out of a server. Where it was rolled out it stays.
op_homelab_secret_remove() {
    local server="$1" path="$2"
    enter
    server "$server"
    within "$path"
    if [ ! -f "${lab}/${server}/${path}.age" ]; then exit 34; fi
    rm -- "${lab}/${server}/${path}.age"
    tidy "${lab}/${server}/${path}.age" "${lab}/${server}"
}

operation "$@"
