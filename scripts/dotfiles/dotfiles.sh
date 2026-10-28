# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

# The dotfiles as chezmoi keeps them: its source folder a git repository, which chezmoi writes into
# the home folder. This script asks chezmoi and git and puts nothing of its own into the dotfiles.

# The repository of the dotfiles, chezmoi's source folder. chezmoi reads its files from a folder
# in it where .chezmoiroot names one. Set by where.
src=''

# A query has no terminal to ask on, and no window may open for one: git and ssh fail instead. It
# reads beside a run and takes no lock it can do without, as git says of a status in the background.
if [ -z "${SM_RUN:-}" ]; then
    export GIT_TERMINAL_PROMPT=0 GIT_ASKPASS='' SSH_ASKPASS_REQUIRE=never GIT_OPTIONAL_LOCKS=0
fi
# Plain http and git:// are refused, also as the target of a redirect.
export GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=protocol.http.allow GIT_CONFIG_VALUE_0=never \
    GIT_CONFIG_KEY_1=protocol.git.allow GIT_CONFIG_VALUE_1=never

# chezmoi without a terminal of its own, which would draw its questions: none is asked.
cm() {
    chezmoi --no-tty --no-pager --color=false --progress=false "$@"
}

# chezmoi's words of a refusal into the details, then the exit for it.
refused() {
    printf '%s\n' "$1" >&2
    exit 47
}

# The repository of the dotfiles.
where() {
    src="$(cm execute-template '{{ .chezmoi.config.sourceDir }}')"
}

# Whether the dotfiles are a work tree of their own, not a bare repository or a folder in another.
repository() {
    local top
    top="$(git -C "$src" rev-parse --show-toplevel 2>/dev/null)" || return 1
    [ "$top" = "$(realpath -- "$src")" ]
}

# Into the dotfiles.
enter() {
    where
    if ! repository; then exit 10; fi
    cd -- "$src"
}

# Whether the dotfiles make chezmoi's configuration from a template, which may ask for values.
configured() {
    [ -n "$(find "$(cm source-path)" -maxdepth 1 -name '.chezmoi.*.tmpl' -print -quit)" ]
}

# Staged as it is added, as a global ignore file naming it, say CLAUDE.md, would keep it out of
# every backup.
track() {
    git add --force -- "$@"
}

# chezmoi's source folder as a repository, made where there is none.
ensure_repository() {
    where
    if [ ! -e "$src" ]; then
        git -c advice.defaultBranchName=false init --quiet -- "$src"
    fi
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

# chezmoi's status, a place of $HOME a line after two letters: what changed here since chezmoi
# wrote it, and what an apply would change, a space for nothing. Folders aside.
drift() {
    local said
    said="$(cm status --exclude=dirs 2>&1)" || refused "$said"
    if [ -n "$said" ]; then printf '%s\n' "$said"; fi
}

# Every place of $HOME the dotfiles keep, a line each: `managed [chezmoi's filter]`.
managed() {
    cm managed --path-style=absolute --include="${1:-files}"
}

# What a setup would apply and a backup would save, a line each. Nothing past the repository
# where it is missing, nothing past the configuration where chezmoi never made it.
op_state() {
    local t counts line said changed=0 due=0 files secrets
    where
    printf 'dir\t%s\nhost\t%s\n' "$src" "$(</proc/sys/kernel/hostname)"
    if [ ! -e "$src" ]; then
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
    if configured && [ ! -f "$(cm execute-template '{{ .chezmoi.configFile }}')" ]; then
        printf 'config\tmissing\n'
        return 0
    fi
    # Templates that do not render say so in the details, and the rest is unknown.
    if ! said="$(cm status --exclude=dirs 2>&1)"; then
        printf '%s\n' "$said" >&2
        printf 'status\tfailed\n'
        return 0
    fi
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        # What changed here stays for a backup: a setup applies only the rest.
        if [ "${line:0:1}" != ' ' ]; then
            changed=$((changed + 1))
        elif [ "${line:1:1}" != ' ' ]; then
            due=$((due + 1))
        fi
    done <<<"$said"
    secrets="$(managed encrypted | grep -c . || allow 1)"
    files="$(managed | grep -c . || allow 1)"
    printf 'files\t%s\nsecrets\t%s\ndue\t%s\nchanged\t%s\n' "$((files - secrets))" "$secrets" "$due" "$changed"
}

# Fetched at the start, so ahead and behind tell what the remote holds now.
op_fetch() {
    where
    if ! repository; then return 0; fi
    enter
    if remote; then fetch_remote; fi
}

# Clones the repository into chezmoi's source folder, or binds the dotfiles to it. Empty, the
# dotfiles stay on this machine only.
op_connect() {
    local url="${1:-}" before said
    case "$url" in
    http://* | git://*) exit 37 ;;
    esac
    where
    if [ ! -e "$src" ]; then
        if [ -z "$url" ]; then return 0; fi
        said="$(git clone --quiet -- "$url" "$src" 2>&1)" || unreached "$said"
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

# This machine as the dotfiles describe it: the latest state, then chezmoi's configuration and its
# apply, on through every error. chezmoi asks in its own words, for a value of its configuration,
# a passphrase, or a file changed here or found here before, so the person answers in the terminal.
op_setup() {
    enter
    if remote; then
        step pull
        integrate
    fi
    step apply
    printf 'terminal\n' >&3
    if configured; then chezmoi init; fi
    chezmoi apply --keep-going --less-interactive
}

# The parts of the dotfiles that changed, for the commit message: `nvim, zshrc`. A file of the home
# folder counts by its program, the folder below .config or .local/share or its own name.
areas() {
    git status --porcelain --untracked-files=all | cut -c4- | awk -F/ '
        # A name as the home folder knows it: without the attributes of chezmoi, dot_ a dot.
        function plain(s) {
            while (sub(/^(create|modify|remove|empty|encrypted|private|readonly|executable|exact|symlink|literal|run|once|onchange|before|after)_/, "", s)) {}
            sub(/^dot_/, ".", s)
            sub(/\.(tmpl|age)$/, "", s)
            return s
        }
        {
            path = plain($1)
            for (i = 2; i <= NF; i++) path = path "/" plain($i)
            sub(/^\.config\//, "", path)
            sub(/^\.local\/share\//, "", path)
            sub(/^\.local\//, "", path)
            split(path, part, "/")
            sub(/^\./, "", part[1])
            print part[1]
        }
    ' | LC_ALL=C sort -u | paste -sd, - | sed 's/,/, /g'
}

# Whether the dotfiles changed a place since chezmoi last wrote it, so a re-add would take that
# change back: what chezmoi would write now against what it wrote then.
moved() {
    local written re='"contentsSHA256"[[:space:]]*:[[:space:]]*"([0-9a-f]{64})"'
    written="$(cm state get --bucket=entryState --key="$1")"
    [[ $written =~ $re ]] || return 0
    [ "${BASH_REMATCH[1]}" != "$(cm cat -- "$1" | sha256sum | cut -d' ' -f1)" ]
}

# What changed here since chezmoi wrote it into the dotfiles, as chezmoi re-add takes it, then
# everything open committed and uploaded. A file whose source is a template waits for chezmoi edit,
# one the dotfiles changed as well for a setup, which asks.
op_backup() {
    local line place source said
    local -a mine=()
    ensure_repository
    enter
    step files
    while IFS= read -r line; do
        if [ "${line:0:2}" != MM ]; then continue; fi
        place="${HOME}/${line:3}"
        source="$(cm source-path -- "$place" 2>/dev/null)" || continue
        if [[ $source == *.tmpl ]]; then
            echo "Changed here, its source a template: ${place}"
        elif moved "$place"; then
            echo "Changed here and in the dotfiles: ${place}"
        else
            mine+=("$place")
        fi
    done < <(drift)
    if [ ${#mine[@]} -gt 0 ]; then
        said="$(cm re-add -- "${mine[@]}" 2>&1)" || refused "$said"
    fi
    if [ -n "$(git status --porcelain)" ]; then
        git var GIT_AUTHOR_IDENT >/dev/null 2>&1 || exit 19
        step commit
        git add -A
        git commit --quiet -m "Back up $(</proc/sys/kernel/hostname): $(areas)"
    fi
    if remote && git rev-parse --verify -q HEAD >/dev/null; then
        step pull
        integrate
        step push
        push
    fi
}

# An XDG folder of this user, where its files are no configuration.
xdg() {
    case "${!1:-}" in
    /*) printf '%s\n' "${!1}" ;;
    *) printf '%s\n' "${HOME}/$2" ;;
    esac
}

# Exits with the reason a path of $HOME cannot go into the dotfiles, if there is one. A folder
# goes with at most 100 files: more is a program's data, not its configuration. chezmoi's own
# folder holds its configuration and keys.
refuse() {
    local path="$1" skip files
    if [[ $path != "$HOME"/?* ]]; then exit 24; fi
    for skip in "$src" "$(xdg XDG_CACHE_HOME .cache)" "$(xdg XDG_STATE_HOME .local/state)" \
        "$(xdg XDG_CONFIG_HOME .config)/chezmoi"; do
        if [[ $path == "$skip" || $path == "$skip"/* ]]; then exit 25; fi
        if [[ $skip == "$path"/* ]]; then exit 26; fi
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

# Whether the dotfiles keep a place already, or something in it.
kept() {
    local place
    while IFS= read -r place; do
        if [[ $place == "$1" || $place == "$1"/* ]]; then return 0; fi
    done < <(managed)
    return 1
}

# A file or folder of $HOME into the dotfiles, as chezmoi adds it with the flags given: `bring
# <path> [flag]`. It stays where it is.
bring() {
    local said
    where
    refuse "$1"
    ensure_repository
    enter
    if kept "$1"; then exit 33; fi
    said="$(cm add "${@:2}" -- "$1" 2>&1)" || refused "$said"
    track "$(cm source-path -- "$1")"
}

# Puts a file or folder of $HOME into the dotfiles.
op_add() {
    bring "$1"
}

# Puts a file or folder of $HOME into the dotfiles encrypted, as chezmoi's configuration names it.
op_secret_add() {
    bring "$1" --encrypt
}

# Removes the folders left empty in the dotfiles above a file, up to the folder given.
tidy() {
    local dir="${1%/*}"
    while [[ $dir == "$2"/* ]] && rmdir -- "$dir" 2>/dev/null; do
        dir="${dir%/*}"
    done
}

# Takes the places the dotfiles keep at a path out of them, `release <path> <files|encrypted>`.
# Each stays in the home folder as it is, and no empty folder stays in the dotfiles, which chezmoi
# would still make.
release() {
    local chosen="${1%/}" place source said found=0
    local -A sealed=()
    while IFS= read -r place; do sealed["$place"]=1; done < <(managed encrypted)
    while IFS= read -r place; do
        if [[ $place != "$chosen" && $place != "$chosen"/* ]]; then continue; fi
        case "$2" in
        encrypted) [ -n "${sealed[$place]:-}" ] || continue ;;
        files) [ -z "${sealed[$place]:-}" ] || continue ;;
        esac
        found=1
        source="$(cm source-path -- "$place")"
        said="$(cm forget --force -- "$place" 2>&1)" || refused "$said"
        tidy "$source" "$src"
    done < <(managed)
    if [ "$found" = 0 ]; then exit 34; fi
}

# Takes a file, or every file of a folder, out of the dotfiles. It stays where it is.
op_remove() {
    enter
    release "$1" files
}

# Takes a secret, or every secret of a folder, out of the dotfiles. Its file stays where it is.
op_secret_remove() {
    enter
    release "$1" encrypted
}

# What the dotfiles keep in the home folder, as `file<TAB>path`, each folder holding nothing else
# as `dir<TAB>path/`, then each secret as `secret<TAB>path`.
op_managed() {
    local place dir entry only
    local -a whole=() secrets=()
    # A folder above a file is 1, one holding nothing but kept files 2.
    local -A files=() folders=() sealed=()
    enter
    while IFS= read -r place; do sealed["$place"]=1; done < <(managed encrypted)
    while IFS= read -r place; do
        if [ -n "${sealed[$place]:-}" ]; then
            secrets+=("$place")
            continue
        fi
        files["$place"]=1
        dir="${place%/*}"
        while [[ $dir == "$HOME"/?* ]]; do
            folders["$dir"]=1
            dir="${dir%/*}"
        done
    done < <(managed)
    # Deepest first, each folder read one level down: its subfolders are judged by then.
    while IFS= read -r dir; do
        only=1
        while IFS= read -r -d '' entry; do
            if [ -d "$entry" ] && [ ! -L "$entry" ]; then
                if [ "${folders[$entry]:-}" != 2 ]; then only=0; fi
            elif [ -z "${files[$entry]:-}" ]; then
                only=0
            fi
        done < <(find "$dir" -mindepth 1 -maxdepth 1 -print0)
        if [ "$only" = 1 ]; then
            folders["$dir"]=2
            whole+=("$dir")
        fi
    done < <(for dir in "${!folders[@]}"; do printf '%s\t%s\n' "${dir//[^\/]/}" "$dir"; done | sort -r | cut -f2-)
    {
        for place in "${!files[@]}"; do printf 'file\t%s\n' "$place"; done
        for dir in "${whole[@]}"; do printf 'dir\t%s/\n' "$dir"; done
    } | LC_ALL=C sort -t $'\t' -k2
    for place in "${secrets[@]}"; do printf 'secret\t%s\n' "$place"; done | LC_ALL=C sort -t $'\t' -k2
}

operation "$@"
