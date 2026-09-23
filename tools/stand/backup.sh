#!/usr/bin/env bash
# Carries a copy of the stand's books off the machine they live on.
#
# The copy `update.sh` takes before every version stays on the stand, which
# covers a bad migration and nothing else: a card that dies takes those copies
# with it. This one lands here, and is read back before it is kept - a copy
# nobody has opened is not a backup.
#
# Nothing on the stand is changed and nothing is stopped: PostgreSQL dumps a
# live database consistently.
#
#   ./tools/stand/backup.sh [directory]
#
# Configuration, from the environment or a `.env` beside the repository:
#
#   AUSTERIS_STAND_HOST=pi                   # the ssh host the stand runs on
#   AUSTERIS_STAND_DIR=/srv/austeris         # where its compose file lives
#   AUSTERIS_BACKUP_TO=./backups             # where copies land here
#   AUSTERIS_BACKUP_KEEP=14                  # how many to keep here
set -euo pipefail

# --- What this stand is -----------------------------------------------------
app=austeris

here="$(cd "$(dirname "$0")/../.." && pwd)"
. "$here/tools/stand/common.sh"

to=${1:-${AUSTERIS_BACKUP_TO:-./backups}}
keep=${AUSTERIS_BACKUP_KEEP:-14}
need AUSTERIS_STAND_HOST "name the ssh host the stand runs on (e.g. pi)"
need AUSTERIS_STAND_DIR "name the directory on that host its compose file lives in (e.g. /srv/$app)"
host=$AUSTERIS_STAND_HOST
dir=$AUSTERIS_STAND_DIR

mkdir -p "$to"
name="$app-$(date -u +%Y-%m-%d).dump"
landing="$to/$name"

# Straight to a file here, so nothing is held in memory on either end.
say "dumping the books on $host"
if ! on_stand "docker compose exec -T db pg_dump -U $app -Fc $app" </dev/null > "$landing.part"; then
    rm -f "$landing.part"
    die "the dump could not be taken on $host: check that the stand is up (docker compose ps) and that $dir is its directory"
fi

# Read back before it gets the name of a backup. A file under the right name
# that turns out to be truncated is worse than no file: it is the one somebody
# reaches for.
if ! check_dump "$landing.part" "$name"; then
    rm -f "$landing.part"
    die "what arrived is not a whole $app dump - the copy was not kept"
fi
mv "$landing.part" "$landing"
say "kept $landing ($(file_size "$landing"))"

# Old copies go by the date in the name. A file that is not one of these is
# left alone: this deletes, and a delete that guesses eventually guesses wrong.
copies=$(ls -1 "$to"/$app-????-??-??.dump 2>/dev/null | sort || true)
count=$(printf '%s' "$copies" | grep -c . || true)
if [ "$count" -gt "$keep" ]; then
    printf '%s\n' "$copies" | head -n "$((count - keep))" | while IFS= read -r old; do
        rm -f "$old"
        say "dropped $(basename "$old")"
    done
fi
