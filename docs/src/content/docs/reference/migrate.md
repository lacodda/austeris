---
title: austeris migrate
description: Applying and rolling back a service's schema.
---

A service migrates its own schema when it starts, so this command exists for the
two times that is not what you want: seeing what a deploy is about to do, and
undoing what one already did.

```console
$ austeris migrate [--service <SERVICE>] [--dry-run] [--undo-to <VERSION>]
```

## Seeing what is pending

```console
$ austeris migrate --dry-run
identity: 1 migration(s) pending
  20260901120001  people and sessions
```

Nothing is applied. Without `--service` every service is planned in turn; a
service that owns no schema says so rather than passing in silence.

## Applying

```console
$ austeris migrate --service identity
identity: 1 migration(s) pending
  20260901120001  people and sessions
identity: applied
```

Running it again reports `up to date` and changes nothing.

## Rolling back

Every migration ships with its reverse, so a release that turns out wrong is
withdrawn by migrating down rather than by restoring a backup and losing
everything written since.

```console
$ austeris migrate --service identity --undo-to 20260901120001
identity: rolled back to 20260901120001
```

The version named is the one to **stop at**, not the one to undo. To undo
everything, pass `-1`:

```console
$ austeris migrate --service identity --undo-to -1
identity: rolled back to -1
```

`--undo-to` requires `--service`: rolling several schemas back at once is never
what someone means. It refuses **before touching anything** if any migration in
the range has no reverse - discovering that halfway through would leave the
newer ones already undone and no way back.
