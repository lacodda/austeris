#!/usr/bin/env bash
# Moves the stand to another published image, with a copy of the books first.
#
# The order is the reason this exists. The copy is taken *before* the new image
# starts, because austeris migrates every schema on the way up: by the time
# anything looks wrong, the migration has run - and a migration that has
# shipped is frozen, so the only way back is the copy.
#
#   ./tools/stand/update.sh v0.7.0     # a release
#   ./tools/stand/update.sh edge       # the image built by hand before a tag
#   ./tools/stand/update.sh            # whatever this checkout is tagged at
#
# Configuration, from the environment or a `.env` beside the repository:
#
#   AUSTERIS_STAND_HOST=pi                   # the ssh host the stand runs on
#   AUSTERIS_STAND_DIR=/srv/austeris         # where its compose file lives
set -euo pipefail

# --- What this stand is -----------------------------------------------------
app=austeris
service=austeris
version_var=AUSTERIS_VERSION

here="$(cd "$(dirname "$0")/../.." && pwd)"
. "$here/tools/stand/common.sh"

need AUSTERIS_STAND_HOST "name the ssh host the stand runs on (e.g. pi)"
need AUSTERIS_STAND_DIR "name the directory on that host its compose file lives in (e.g. /srv/$app)"
host=$AUSTERIS_STAND_HOST
dir=$AUSTERIS_STAND_DIR

# A version is named rather than guessed from the manifest: the version in
# `Cargo.toml` is one that is *going* to ship, and its image exists only once
# the tag has been built. A release is written with its `v`; the image tag is
# the number. Anything else (`edge`, `sha-1a2b3c4`) is an image tag as it is.
wanted=${1:-$(git -C "$here" describe --tags --exact-match 2>/dev/null || true)}
[ -n "$wanted" ] || die "name the version to move to (v0.7.0, or edge), or run this from a tagged checkout"
case "$wanted" in
    v[0-9]*) tag=${wanted#v} ;;
    *) tag=$wanted ;;
esac
image="ghcr.io/lacodda/$app:$tag"

# The image has to exist before the stand is touched for it: checked from the
# stand, which is the machine that has to pull it.
say "looking for $image"
on_stand "docker manifest inspect '$image' >/dev/null 2>&1" </dev/null \
    || die "there is no image $image yet: the build may still be running, or the tag was never pushed"

# --- The copy, before anything moves ----------------------------------------
# Taken from the running database and kept on the stand: this is what a
# rollback restores, and it has to be on the machine the rollback happens on.
# `tools/stand/backup.sh` is what carries copies off the machine.
stamp=$(date -u +%Y%m%dT%H%M%SZ)
aside="backups/$app-before-$tag-$stamp.dump"
say "copying the books aside on $host: $aside"
# PostgreSQL makes a consistent dump of a live database, so nothing is stopped
# for it - the stand answers until the new image replaces it.
if ! on_stand "mkdir -p backups && docker compose exec -T db pg_dump -U $app -Fc $app > '$aside' && test -s '$aside'" </dev/null; then
    on_stand "rm -f '$aside'" </dev/null || true
    die "the copy could not be taken; nothing was changed - a version must not move without something to move back to"
fi
tables=$(on_stand "docker compose exec -T db pg_restore --list < '$aside' | grep -c 'TABLE DATA'" </dev/null || true)
[ "${tables:-0}" -gt 0 ] || die "the copy $aside cannot be read back; nothing was changed"
say "copied aside: $tables tables; a rollback restores $aside"

# --- The version ------------------------------------------------------------
# Written into `.env` on the stand rather than passed on the command line, so a
# later `docker compose up -d` by hand brings up this version and not whatever
# `latest` has become.
say "setting $version_var=$tag in $dir/.env"
on_stand "touch .env && sed -i '/^$version_var=/d' .env && echo '$version_var=$tag' >> .env" </dev/null

say "pulling $image"
on_stand "docker compose pull $service" </dev/null
say "starting it"
on_stand "docker compose up -d" </dev/null

# --- Is it the version that was asked for, and is it well? ------------------
# Ready, not merely started: the public port opens only after every schema is
# migrated, so /readyz answering means the migration went through.
say "waiting for the stand to be ready"
ready=
for _ in $(seq 60); do
    if on_stand "docker compose exec -T $service curl -fsS http://127.0.0.1:8080/readyz" </dev/null >/dev/null 2>&1; then
        ready=1
        break
    fi
    sleep 2
done
running=$(on_stand "docker compose exec -T $service $app --version" </dev/null 2>/dev/null || true)

if [ -n "$ready" ]; then
    say "the stand is up: $running"
else
    say "the stand did not become ready; its log:"
    on_stand "docker compose logs --tail 40 $service" </dev/null || true
    say "to go back: ./tools/stand/restore.sh with $aside, and the previous $version_var in .env"
    exit 1
fi
