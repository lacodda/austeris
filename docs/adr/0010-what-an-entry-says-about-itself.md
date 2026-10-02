# 10. What an entry says about itself

Date: 2026-10-02

## Status

Accepted. Extends [0007](0007-entries-balance-by-construction.md), which made an
entry a header and its lines, and [0009](0009-conversions-and-official-rates.md),
whose conversions a held payment works out again when it posts. This is the last
change to the shape of the ledger before 1.0; what follows only adds to it.

## Context

The lines of an entry say where money went. Four things a person asks about it
were nowhere: who it was paid to, where they were, which of their own plans it
belonged to across categories, and whether the bank had posted it yet.

Each could be put in several places, and the places differ in what they cost to
change later. A counterparty can be a line's or the entry's; a place can be two
columns or a row; a hold can be a flag, a date, or a state; and an entry's
references to a person's own rows can be checked by the code that writes or
held by the schema.

## Decision

**Counterparty, place, tags and state belong to the entry, not to a line.** A
receipt is one shop however many categories it is split across; a transfer
between a person's own accounts has no counterparty at all; a card payment is
held or posted as a whole. Tags on single lines, should they be wanted, are a
table added beside the entry's - the effective tags of a line are then both -
and that is growth, not a change of shape.

**A counterparty and a place are rows, not text copied onto entries.** Renaming
"Asuncion" to "Asunción", or a shop that changed its sign, changes one row
rather than every entry that names it. A counterparty is typed in a line as one
word, so it is matched by a key derived in SQL - the name in lower case without
spaces or punctuation - and that one function is both the generated column and
every lookup. A place is found by what it is, a country and a city, and created
the first time.

**A payment is held or cleared, stated, with the day it cleared.** Not a flag:
a balance as of a past day must read the same before and after the payment
posted, so the day it posted is kept. Not a date that is empty while held: an
empty date read as "held" is a meaning every reader has to know, and a date
lost by mistake would read as a payment still held. The state is an enum with a
check binding it to the date; reconciling against a statement will add a state,
not a column.

A balance is what the bank has posted by the end of the day. What is
**available** is that less the payments held on it. Money held on its way in is
not added: it is not available until it arrives, which is how banks count it.

**Every new reference names its owner.** The foreign keys from an entry to its
counterparty, place and tags, and from a counterparty to its usual category,
are on `(owner_id, id)`. A row of one person's pointing at another's is not
refused by the code that usually writes; it cannot be stored.

**Totals are one query over the lines on categories,** grouped by category, tag,
counterparty, place or country. What money was for is the category side of an
entry, so a transfer moves nothing there and an exchange's fee is spending like
any other. Each group is reported in every currency it moved in, never added
across them; converting a flow honestly needs each line's own day's rate, which
is a later step rather than a rate of today's applied to last year.

## Consequences

A typed line grows three marks: `@` names a counterparty (a word starting with a
digit is still a rate), `#` a tag, a leading `~` a hold. A counterparty named
for the first time is created with the line's category as its usual one, so
"this shop is groceries" needs no rules engine; the answer says it was created,
so a mistyped name is seen the first time.

Posting a hold for another amount rebuilds the entry's lines in the same
transaction. Where the entry's shape does not say which part changed - two
accounts, a split - it is refused and recorded again instead.

A schema holding held payments, counterparties, places or tags refuses to roll
back to the previous version, which could express none of them.
