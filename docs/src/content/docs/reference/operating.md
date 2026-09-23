---
title: Operating a stand
description: Moving an installation to a new version, and keeping copies of its books.
---

An installation on another machine - a Raspberry Pi at home, say - is run from
your own over `ssh`. Three scripts in `tools/stand/` do the three things that
must not be done from memory. They need `ssh` access to the machine and the
directory its compose file lives in:

```ini
# .env beside the repository - read by these scripts, never by austeris
AUSTERIS_STAND_HOST=pi
AUSTERIS_STAND_DIR=/srv/austeris
```

## Moving to a new version

```console
$ ./tools/stand/update.sh v0.7.0
```

Checks the image exists, dumps the database aside on the stand, writes the
version into the stand's `.env`, pulls, starts, and waits until the stand is
ready. The dump comes first because austeris migrates its schemas on the way
up: if anything looks wrong afterwards, the migration has already run, and the
dump is the way back.

`update.sh edge` moves to the image built by hand from `main` before a release -
how a version is tried on a real machine before it is tagged.

## A copy off the machine

```console
$ ./tools/stand/backup.sh
backup: checked austeris-2026-09-23.dump: whole, 9 tables
backup: kept ./backups/austeris-2026-09-23.dump (84 kB)
```

The dumps `update.sh` takes stay on the stand, which covers a bad migration and
nothing else. This one is carried here and read back by `pg_restore` before it
is kept; the newest fourteen are kept (`AUSTERIS_BACKUP_KEEP`,
`AUSTERIS_BACKUP_TO`).

## Putting the books back

```console
$ ./tools/stand/restore.sh backups/austeris-2026-09-23.dump
```

Into an empty database only: the stand's compose file and `.env` must be there,
and austeris must not have started on it yet - it would migrate the database
and create an account, and the dump would collide with both. The script starts
the database alone, checks the dump, refuses a database that already holds
books, restores, and then starts austeris.
