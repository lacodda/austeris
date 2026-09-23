# Shared by the stand scripts. Sourced, never run.
#
# Taken from rhapsod's `tools/stand/`, the line's shared shape for a stand on
# the Pi: the same words for a refusal, a step said before it is done, and a
# `.env` beside the repository for the values a person would otherwise retype.
# What differs is the database: rhapsod keeps a SQLite file in a volume,
# austeris keeps PostgreSQL in a container of its own, so a copy is a
# `pg_dump` rather than a file copy - and checking a copy means asking
# `pg_restore` to read it back, not asking SQLite.
#
# POSIX shell throughout: this runs on a Pi, in Git Bash on Windows, and in
# whatever /bin/sh a rescue image has.

# The name of the script that sourced this, for the prefix on every line.
_stand_who=${0##*/}
_stand_who=${_stand_who%.sh}

say() {
    echo "$_stand_who: $1"
}

# A refusal is one line naming what to fix, on stderr, and then nothing.
die() {
    echo "$_stand_who: $1" >&2
    exit 1
}

# A variable that must be set, with the sentence that says what to put in it.
# Checked before anything is copied or started: a script that fails halfway
# across a network has already changed the stand.
need() {
    eval "value=\${$1:-}"
    [ -n "$value" ] || die "$1 is not set: $2"
}

# A `.env` beside the repository. Values already in the environment win, which
# is what makes a one-off run against another machine a prefix on the command
# line rather than an edit.
#
# Only this product's stand variables are read, and only ones shaped like a
# name: a `.env` is a file people put things in, and sourcing it would run
# whatever is in there.
load_env() {
    file="$1"
    [ -f "$file" ] || return 0
    while IFS= read -r line || [ -n "$line" ]; do
        case "$line" in
            AUSTERIS_STAND_*=* | AUSTERIS_BACKUP_*=* | AUSTERIS_YES=*) ;;
            *) continue ;;
        esac
        name=${line%%=*}
        value=${line#*=}
        value=${value%\"}
        value=${value#\"}
        value=${value%\'}
        value=${value#\'}
        eval "export ${name}=\"\${${name}:-\$value}\""
    done < "$file"
}

# Runs a command in the stand's directory on its host. stdin is passed through,
# which is how a dump travels to `pg_restore` without a temporary file.
on_stand() {
    ssh "$host" "cd '$dir' && $1"
}

# Whether a dump is a whole dump of this product's database.
#
# Read back by `pg_restore --list` in the stand's own database container - the
# one place a matching `pg_restore` is certain to exist - so the bytes checked
# are the bytes that arrived, not the ones that left. The list must name the
# ledger's entries table: a well-formed dump of some other database passes the
# format check and would restore a stand to nothing.
check_dump() {
    file="$1"
    shown=${2:-$(basename "$file")}
    [ -s "$file" ] || { echo "$_stand_who: $file is empty" >&2; return 1; }

    listing=$(on_stand "docker compose exec -T db pg_restore --list" < "$file" 2>&1) || {
        echo "$_stand_who: $shown cannot be read back: $listing" >&2
        return 1
    }
    case "$listing" in
        *"TABLE DATA ledger entries"*) ;;
        *)
            echo "$_stand_who: $shown is readable but holds no ledger - not an $app dump" >&2
            return 1
            ;;
    esac
    tables=$(printf '%s\n' "$listing" | grep -c 'TABLE DATA' || true)
    say "checked $shown: whole, $tables tables"
}

# A file's size in something a person reads, without depending on which `stat`
# or `du` this machine has.
file_size() {
    bytes=$(wc -c < "$1" | tr -d ' ')
    if [ "$bytes" -ge 1048576 ]; then
        echo "$((bytes / 1048576)) MB"
    elif [ "$bytes" -ge 1024 ]; then
        echo "$((bytes / 1024)) kB"
    else
        echo "$bytes bytes"
    fi
}

# Asks before something that cannot be taken back. `AUSTERIS_YES=1` answers for
# a non-interactive shell; without it, no answer means no.
confirm() {
    case "${AUSTERIS_YES:-}" in
        1 | yes | true) return 0 ;;
    esac
    printf '%s: %s [y/N] ' "$_stand_who" "$1"
    read -r answer </dev/tty 2>/dev/null || answer=
    case "$answer" in
        y | Y | yes | YES) return 0 ;;
        *) return 1 ;;
    esac
}

load_env "$here/.env"
