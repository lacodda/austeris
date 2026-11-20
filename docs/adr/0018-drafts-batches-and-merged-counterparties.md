# 18. Drafts, batches, merged counterparties and a trash for trades

Date: 2026-11-20

## Status

Accepted. Builds on [0007](0007-entries-balance-by-construction.md),
[0010](0010-what-an-entry-says-about-itself.md),
[0013](0013-the-memory-of-an-entry.md) and
[0015](0015-the-portfolio-and-the-money-it-moves.md).

## Context

The books were kept one entry at a time, and sorting them was slow in four
places. At a till there is time for the amount and not for the category, and an
entry without one was refused - so it was not recorded at all, or recorded under
whatever category came first, which is worse because nothing says it was a
guess. A month of receipts filed under the wrong category, or against the card
instead of the cash, was mended entry by entry. A typo in a shop's name after
`@` made a second counterparty, and its entries could only be moved one by one.
And a trade deleted by mistake was gone: its history kept what it was, but
nothing could bring it back, as the ledger's trash brings back an entry.

## Decision

**A draft is a line side of its own.** A fifth `line_side`, `unsorted`: money
spent or earned on what is not said yet. It names nothing, as a conversion does
not, and balances like any other line; giving it a category turns it into a
category line. A side rather than a category line with no category, so a query
reading category lines cannot count a draft as spending on nothing in particular
by accident, and a client reading the line is told what it is. Totals count it
by its sign - money arriving on the unsorted side was spent, money leaving it
was earned - under the group with no id, so a month of drafts is not a month in
which nothing was spent. A typed line that says nothing it was for, or names only
a counterparty with no usual category, records one instead of being refused; a
word that names no category is still refused, because it was said and meant
something.

**A batch is one transaction.** `POST /entries/batch` gives many entries a
category, moves their money to another account, or labels them, and does all of
it or none of it: the entries are locked in one order, each one's history is
written by the same triggers as a single change (ADR 0013), and a refusal names
what refused it. A category given in a batch reaches unsorted lines and lines on
the person's own categories, never a category the ledger or a module keeps -
an exchange's fees stay fees. Money is moved only between accounts of the same
currency: another currency needs a rate nobody stated. An entry a module keeps
is refused rather than skipped, since its money changes in the module.

**A merged counterparty leaves the way it was typed behind.** Merging moves the
entries and the templates, completes the one kept with what it lacked, and
deletes the other - and keeps the merged one's key in `counterparty_aliases`, so
the typo typed again finds the right counterparty instead of making a third.
One SQL function, `counterparty_typed`, reads a typed name for every path - a
line, a receipt, an entry restored from the trash. A name given to a
counterparty on purpose takes the key back from an alias: a shop really called
that is a new shop. Two counterparties with different tax numbers are refused,
being two businesses.

**A deleted trade's trash is its history.** As for an entry, nothing is marked
deleted: the last revision of a deleted trade holds it as it was, and the trash
is those revisions for thirty days - one number for every trash
(`austeris_common::trash::KEEP_DAYS`). Restoring writes the trade back under its
own id, key, day and time of recording, checked as a new trade is, and books its
money again under the key it was booked under, `portfolio:trade:<id>`; the
ledger let go of that entry when the trade was deleted, so the key is free and
the entry is the trade's again. Its history goes on with a `restored` revision,
which the trigger writes when a trade is inserted under an id that has a
history. A trade whose units were sold since, or whose key a trade recorded
since has taken, stays in the trash.

## Consequences

- A screen can record a payment with nothing but an amount, and count the
  drafts it has to come back to: `GET /entries/unsorted`, `?unsorted=true`.
- The quick line's `line.no_category`, `line.no_usual_category` and
  `line.unknown_counterparty` are gone; a client that relied on the refusal gets
  a draft.
- Rolling back over a draft, or a trade that came back from the trash, is
  refused: the previous schema cannot say either. A value cannot leave a
  PostgreSQL enum, so the words stay in their types, unused.
- Merging is not undone by the trash: it changes who many entries name, and
  each entry's history says so.
