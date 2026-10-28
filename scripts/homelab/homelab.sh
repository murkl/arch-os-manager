# shellcheck shell=bash source=../lib/lib.sh
. "${BASH_SOURCE[0]%/*}/../lib/lib.sh" || exit 1

# Arcane through its REST API, with a key of the manager's own from the keyring. It travels over
# https only and reaches curl through a file descriptor, never on a command line.

# What the key may do and nothing more: read the servers, the stacks and their syncs, and deploy
# and sync a stack.
rights=(environments:list projects:list projects:read projects:deploy gitops:list gitops:read
    gitops:sync)

# The header of a request: the key, or the session while the key is made.
auth=''
# The status of the last request, by request.
status=''

# The address of Arcane, https only, without a slash at the end.
address() {
    case "$1" in
    https://?*) ;;
    *) exit 14 ;;
    esac
    url="${1%/}"
}

# An id of Arcane in a path: what its ids are made of, nothing that leaves the path.
id() {
    if [[ ! $1 =~ ^[A-Za-z0-9-]+$ ]]; then exit 18; fi
}

# The key of the address into the header of every request.
unlock() {
    local key
    key="$(secret-tool lookup service system-manager url "$url" 2>/dev/null)" || exit 13
    if [ -z "$key" ]; then exit 13; fi
    auth="X-Api-Key: ${key}"
}

# A request to Arcane: `request <method> <path> [curl options]`, the answer in $work/body and its
# status in $status. One that does not reach Arcane exits 10. With the key, a refusal exits 11 and
# a missing right 12. The session's answers are the caller's.
request() {
    local method="$1" path="$2"
    shift 2
    scratch
    status="$(fetch --no-fail -X "$method" -o "${work}/body" -w '%{http_code}' \
        -H @<(printf '%s\n' "${auth:-Accept: application/json}") "$@" "${url}/api${path}")" ||
        exit 10
    if [[ $auth != X-Api-Key:* ]]; then return 0; fi
    case "$status" in
    401) exit 11 ;;
    403) exit 12 ;;
    esac
}

# A request with a JSON body, which goes through a file descriptor too: `send <method> <path>
# <json>`.
send() {
    request "$1" "$2" -H 'Content-Type: application/json' --data-binary @<(printf '%s' "$3")
}

# Whether the last answer holds a key with a literal value: `holds <key> <true|false|null>`.
holds() {
    [[ $(<"${work}/body") =~ \"$1\"[[:space:]]*:[[:space:]]*$2 ]]
}

# What Arcane streams as it acts, a JSON object a line, shown as text: each log line, and an
# error exits 18. The status follows the stream on a line of its own.
stream() {
    local line text last='' failed=0
    while IFS= read -r line; do
        last="$line"
        text="$(value error "$line")"
        if [ -n "$text" ]; then
            printf '%s\n' "$text" >&2
            failed=1
        fi
        text="$(value log "$line")"
        if [ -n "$text" ]; then printf '%s\n' "$text"; fi
    done < <(fetch --no-fail -N --max-time 3600 -X "$1" -w '\n%{http_code}\n' \
        -H @<(printf '%s\n' "$auth") "${url}/api$2" || echo 000)
    case "$last" in
    000) exit 10 ;;
    401) exit 11 ;;
    403) exit 12 ;;
    2??) ;;
    *) exit 18 ;;
    esac
    if [ "$failed" = 1 ]; then exit 18; fi
}

# A text as a JSON string. Of the control characters only tab and the line breaks go.
json() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//$'\t'/\\t}"
    s="${s//$'\n'/\\n}"
    s="${s//$'\r'/\\r}"
    if [[ $s =~ [$'\x01'-$'\x1f'] ]]; then exit 19; fi
    printf '"%s"' "$s"
}

# A JSON string decoded, its quotes taken off before.
unjson() {
    local s="$1" out='' c
    while [[ $s =~ ^([^\\]*)\\(.)(.*)$ ]]; do
        out+="${BASH_REMATCH[1]}"
        c="${BASH_REMATCH[2]}"
        s="${BASH_REMATCH[3]}"
        case "$c" in
        n) out+=$'\n' ;;
        t) out+=$'\t' ;;
        u)
            if [[ $s =~ ^([0-9A-Fa-f]{4})(.*)$ ]]; then
                printf -v c '%b' "\\u${BASH_REMATCH[1]}"
                out+="$c"
                s="${BASH_REMATCH[2]}"
            fi
            ;;
        r | b | f) ;;
        *) out+="$c" ;;
        esac
    done
    printf '%s' "${out}${s}"
}

# The first string a key holds in a JSON text, decoded, nothing where it holds none: `value <key>
# <json>`.
value() {
    local re="\"$1\"[[:space:]]*:[[:space:]]*\"(([\\].|[^\"\\])*)\""
    if [[ $2 =~ $re ]]; then unjson "${BASH_REMATCH[1]}"; fi
}

# What Arcane said of a refusal in the last answer, a line each, into the details.
explain() {
    local key text
    for key in error message detail lastSyncError; do
        text="$(value "$key" "$(<"${work}/body")")"
        if [ -n "$text" ]; then printf '%s\n' "$text" >&2; fi
    done
}

# Each path's answer, apart by a NUL. Where Arcane answers with a refusal, as for a server it does
# not reach, the answer is empty and the refusal goes into the details.
op_get() {
    local path
    address "$1"
    shift
    unlock
    for path; do
        request GET "$path"
        case "$status" in
        2??) cat -- "${work}/body" ;;
        *) explain ;;
        esac
        printf '\0'
    done
}

# A key of the manager's own, made with the person's name and password and kept in the keyring.
# One it made before is deleted. The session ends before the key is checked, and a key that may
# list the keys of Arcane is dropped, as it may do more than it needs.
op_login() {
    local user password token old key keyid grants='' right
    address "$1"
    read -rp "Username for '${url}': " user
    read -rsp "Password for '${url}': " password
    echo
    send POST /auth/login "{\"username\":$(json "$user"),\"password\":$(json "$password")}"
    case "$status" in
    2??) ;;
    400 | 401) exit 16 ;;
    *) exit 18 ;;
    esac
    token="$(value token "$(<"${work}/body")")"
    if [ -z "$token" ]; then exit 18; fi
    auth="Authorization: Bearer ${token}"
    if holds requiresPasswordChange true; then
        request POST /auth/logout
        exit 15
    fi
    old="$(secret-tool search service system-manager url "$url" 2>/dev/null |
        sed -n 's/^attribute\.key-id = //p')" || old=''
    if [[ $old =~ ^[A-Za-z0-9-]+$ ]]; then request DELETE "/api-keys/${old}"; fi
    for right in "${rights[@]}"; do grants+="{\"permission\":$(json "$right")},"; done
    send POST /api-keys "{\"name\":$(json "System Manager on $(</proc/sys/kernel/hostname)"),\"permissions\":[${grants%,}]}"
    key="$(value key "$(<"${work}/body")")"
    keyid="$(value id "$(<"${work}/body")")"
    request POST /auth/logout
    if [[ ! $key =~ ^arc_ ]]; then exit 18; fi
    auth="X-Api-Key: ${key}"
    request GET /environments
    if [[ $status != 2?? ]]; then exit 18; fi
    # In a subshell, as the right the key lacks exits 12 there.
    if (request GET /api-keys && [[ $status == 2?? ]]); then exit 17; fi
    printf '%s' "$key" | secret-tool store --label="System Manager: ${url}" \
        service system-manager url "$url" key-id "$keyid"
}

# The key out of the keyring. Arcane keeps it until it is deleted there.
op_logout() {
    address "$1"
    secret-tool clear service system-manager url "$url"
}

# A stack's sync run now: `sync <url> <server> <sync>`.
op_sync() {
    address "$1"
    id "$2"
    id "$3"
    unlock
    request POST "/environments/$2/gitops-syncs/$3/sync"
    if [[ $status != 2?? ]] || holds success false; then
        explain
        exit 18
    fi
}

# The newest images of a stack's tags, and each service whose image changed started anew:
# `update <url> <server> <stack>`.
op_update() {
    address "$1"
    id "$2"
    id "$3"
    unlock
    step pull
    stream POST "/environments/$2/projects/$3/pull"
    step up
    stream POST "/environments/$2/projects/$3/up"
}

# Every service of a stack made anew: `redeploy <url> <server> <stack>`.
op_redeploy() {
    address "$1"
    id "$2"
    id "$3"
    unlock
    stream POST "/environments/$2/projects/$3/redeploy"
}

operation "$@"
