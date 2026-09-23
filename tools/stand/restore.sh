#!/usr/bin/env bash
# Puts a copy of the books back into a stand whose database is empty.
#
# A stand is three things and only one is irreplaceable: the image is pulled
# again, the compose file and `.env` are written again, and the books exist
# nowhere but in a dump. So this is about one file - and about putting it in
# before austeris first starts, because austeris on an empty database migrates
# it and creates an account, and a dump restored over that collides with both.
#
#   ./tools/stand/restore.sh backups/austeris-2026-09-23.dump
#
# Configuration, from the environment or a `.env` beside the repository:
#
#   AUSTERIS_STAND_HOST=pi                   # the ssh host of the stand
#   AUSTERIS_STAND_DIR=/srv/austeris         # where its compose file lives
#
# The compose file and its `.env` must already be there (`turnout deploy`, or
# by hand from docker-compose.install.yml). This does not write them: the
# password in `.env` is the stand's own and belongs to nobody's repository.
set -euo pipefail

# --- What this stand is -----------------------------------------------------
app=austeris
service=austeris

here="$(cd "$(dirname "$0")/../.." && pwd)"
. "$here/tools/stand/common.sh"

need AUSTERIS_STAND_HOST "name the ssh host of the stand (e.g. pi)"
need AUSTERIS_STAND_DIR "name the directory on that host its compose file lives in (e.g. /srv/$app)"
host=$AUSTERIS_STAND_HOST
dir=$AUSTERIS_STAND_DIR

file=${1:-}
[ -n "$file" ] || die "name the dump to restore (e.g. backups/$app-2026-09-23.dump)"
[ -f "$file" ] || die "$file does not exist"

on_stand "test -f docker-compose.yml && test -f .env" </dev/null \
    || die "$host:$dir has no docker-compose.yml and .env yet: put the stand's configuration there first"

# Only the database: austeris must not start before the books are in.
say "starting the database alone"
on_stand "docker compose stop $service >/dev/null 2>&1 || true; docker compose up -d db" </dev/null
for _ in $(seq 30); do
    on_stand "docker compose exec -T db pg_isready -U $app -d $app" </dev/null >/dev/null 2>&1 && break
    sleep 2
done

check_dump "$file"

# Refused on a database that already holds books. Restoring over them is a
# merge nobody asked for, and dropping them first is a delete this script has
# no business guessing at.
held=$(on_stand "docker compose exec -T db psql -U $app -d $app -tAc \"SELECT count(*) FROM information_schema.schemata WHERE schema_name IN ('identity','ledger','market')\"" </dev/null | tr -d '[:space:]')
if [ "${held:-0}" != "0" ]; then
    die "the database on $host already holds $app schemas; restore into an empty one (docker compose down -v removes the old volume - deliberately, by hand)"
fi

confirm "restore $(basename "$file") into the database on $host?" || die "nothing was restored"

say "restoring $(basename "$file")"
on_stand "docker compose exec -T db pg_restore -U $app -d $app --no-owner --exit-on-error" < "$file"

say "starting $app"
on_stand "docker compose up -d" </dev/null
for _ in $(seq 60); do
    if on_stand "docker compose exec -T $service curl -fsS http://127.0.0.1:8080/readyz" </dev/null >/dev/null 2>&1; then
        say "the stand is up with the restored books."
        exit 0
    fi
    sleep 2
done
on_stand "docker compose logs --tail 40 $service" </dev/null || true
die "the books are in, but $app did not become ready - its log is above"
