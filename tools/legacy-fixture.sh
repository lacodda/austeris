#!/usr/bin/env bash
# Makes crates/austeris/tests/fixtures/legacy-2025.sql: the 2025 tracker's
# database as pg_dump writes it - its schema from its own migrations under the
# `legacy-2025` tag, filled with the made-up records of legacy-2025-data.sql.
#
# The server is the tracker's own (PostgreSQL 17.5, in UTC, as its compose file
# ran it); the dump is today's pg_dump, as a person moving off it would run.
# Needs Docker and the `legacy-2025` tag.
set -euo pipefail
cd "$(dirname "$0")/.."

fixtures=crates/austeris/tests/fixtures
name=austeris-legacy-fixture
docker rm -f "$name" >/dev/null 2>&1 || true
docker run -d --name "$name" -e POSTGRES_USER=tracker -e POSTGRES_PASSWORD=tracker -e POSTGRES_DB=austeris -e TZ=UTC postgres:17.5-alpine >/dev/null
trap 'docker rm -f "$name" >/dev/null' EXIT

# Over TCP: while the image initialises, only its socket answers.
for _ in $(seq 60); do
    docker exec "$name" pg_isready -h 127.0.0.1 -U tracker -d austeris >/dev/null 2>&1 && break
    sleep 1
done

sql() { docker exec -i "$name" psql -v ON_ERROR_STOP=1 -q -U tracker -d austeris; }
for migration in $(git ls-tree --name-only legacy-2025 migrations/ | sort); do
    git show "legacy-2025:$migration" | sql
done
sql < "$fixtures/legacy-2025-data.sql"

docker run --rm --network "container:$name" -e PGPASSWORD=tracker postgres:18-alpine \
    pg_dump -h 127.0.0.1 -U tracker -d austeris > "$fixtures/legacy-2025.sql"
echo "wrote $fixtures/legacy-2025.sql"
