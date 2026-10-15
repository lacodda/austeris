# 13. The memory of an entry: its history, its files and the trash

Date: 2026-10-15

## Status

Accepted. Builds on [0004](0004-money-as-numeric-decimal.md),
[0007](0007-entries-balance-by-construction.md) and
[0010](0010-what-an-entry-says-about-itself.md).

## Context

From 0.11 the books are real, and a balance that moved has to be explainable:
which entry moved it, when, by whose hand, and what it said before. A receipt
belongs beside the entry it proves, and a deletion by mistake must not need a
backup to undo. The three are one question - what an entry remembers - and
they share one record.

## Decision

**The database keeps the history, for every writer.** `ledger.entry_revisions`
has one row per entry per transaction that touched it - `UNIQUE (entry_id,
txid)` on `pg_current_xact_id()` - with the action (`created`, `changed`,
`deleted`, `restored`), who did it, when, and the entry as it was **before**:
a `jsonb` snapshot built by one SQL function, `entry_snapshot`, of the header,
the lines with their ids, the tags with their ids and names, the counterparty,
the place and the files. The state after a revision is the one before the next
revision, or the entry as it is now, read through the same function. Triggers
written `BEFORE` on `entries`, `entry_lines`, `entry_tags` and `attachments`
take the snapshot on the first touch of a transaction, so posting a hold for
another amount - a header and every line rewritten - is one revision. A tag's
deletion is remembered by a trigger on `tags` itself, because by the time the
cascade reaches `entry_tags` the tag is gone. A deferred trigger drops, at
`COMMIT`, a change that left the entry as it was - tags replaced by the same
tags, a form saved untouched.

**Who is said by the service and checked by the database.** Every write a
service makes runs in a transaction opened by `austeris_common::Actor`, which
sets the transaction-local `austeris.actor` to `person:<id>` or
`module:<name>`. Every austeris connection names itself `austeris`
(`application_name`), and a write to an entry from that connection with no
actor is refused: a path of the service that forgot to say whose write it is
fails where it is tested, rather than leaving a history that blames nobody. A
write from any other connection - psql, a repair script - is recorded as
`outside`. Recording in Rust, beside the code that usually writes, was the
alternative and was not taken: like the balance rule (0007), a rule enforced
only by the usual writer lasts until something else writes, and a history
with holes exactly where something unusual happened is not a history.

**Deleting really deletes; the trash is the history's last word.** A deleted
entry is gone from `entries` and its lines with it - from every balance, total
and list, none of which has a filter to forget. Its last revision, `deleted`,
holds what it was, and restoring writes that back under the same ids: the
entry's, its lines', its first day of being recorded and a module's key. A
counterparty, place or tag deleted meanwhile is found or made again by name.
A `deleted_at` column was the alternative and was not taken: it makes the
wrong state expressible - any query that forgets the condition counts deleted
money - where this makes it unstorable. A person's trash operations take an
advisory lock first; a row lock on the revision does not serialise two
restores, since the first writes the entry back without changing that row.

**Files live in the database.** `ledger.attachments` keeps the bytes (`bytea`,
`STORAGE EXTERNAL`, ten megabytes at most) with the name, the size, a SHA-256
and what the bytes are, read from them: JPEG, PNG, WebP, HEIC/HEIF, PDF, or an
XML document - never the uploader's word for it. Files on a volume were the
alternative and were not taken: a file and its entry then go to the trash and
come back in two places, a dump taken before an upgrade misses them, and there
is a second thing to back up. An attachment has no foreign key to `entries`
on purpose: it waits for a deleted entry and is on it again when the entry is
restored, with nothing copied; a trigger holds that it arrives on a live entry
of the same person. A file taken off an entry goes to the trash too
(`removed_at`). Content is served with `X-Content-Type-Options: nosniff` and
its read type; an XML document only as a download, under `CSP: sandbox`, since
one in the XHTML namespace is a page that would run with the session.

**Thirty days, then nothing.** `trash::KEEP_DAYS` is how long a deleted entry
or a removed file can come back. An hourly sweep in the ledger's process
erases what has waited longer: an entry with its whole history and its files.
The history of an entry in the books is kept for as long as the entry is.
Erasing names no actor on purpose - it touches no live entry, and the guard
above refuses it if a fault ever pointed it at one.

## Consequences

- A migration that rewrites entries either sets `austeris.actor` to
  `module:migration` or turns the history triggers off for the fill, saying
  why; otherwise it is refused like any other write that names nobody.
- The history of changes begins with 0.12. Entries already recorded get a
  `created` revision on their own day, by the person or by the module that
  signed them - the only two writers there were - and nothing invented about
  what changed in them before.
- The trades of `portfolio` (0.14) keep their history the same way, in their
  own schema, with the same `Actor`.
- Rolling the schema back over a history of changes, the trash or any file is
  refused: the previous schema would drop them.
