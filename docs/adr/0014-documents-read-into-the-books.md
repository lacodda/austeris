# 14. Documents read into the books: the imports module

Date: 2026-10-20

## Status

Accepted. Builds on [0001](0001-microservices-with-guardrails.md),
[0010](0010-what-an-entry-says-about-itself.md) and
[0013](0013-the-memory-of-an-entry.md).

## Context

In Paraguay every sale is documented to SIFEN, the tax authority's system of
electronic documents, and the buyer can have the document itself: an XML file
the shop e-mails, saying who sold, when, every item and how it was paid. It is
the most common way money leaves a household there, and typing it in is the
price of keeping books that most people stop paying in the second week. An
exchange office hands over a paper slip instead; its two amounts and its rate
are the exchange.

Reading documents is not the ledger's business. The ledger knows accounts,
categories and entries (ADR 0001); a Paraguayan tax format, and after it a
bank's CSV and a broker's statement, are what a module reads and turns into
entries the ledger understands.

## Decision

**A module of its own, `imports`**, with its own schema and prefix
(`/api/v1/imports`), born with its switch: `AUSTERIS_IMPORTS=off` leaves it
unstarted, the gateway answers its paths with `module.switched_off`, and
`/api/v1/modules` tells a screen not to offer it. It posts through
`ledger.v1.PostEntry` like any module, signing its entries `imports`.

**`ledger.v1` grows, additively.** `PostEntry` carries a counterparty (named,
with a tax number and the category it is usually for), a place, tags, a hold,
and the files the entry was read from - all written in the entry's own
transaction, so an entry never exists without its receipt. `ListAccounts`,
`FindCounterparty` and `RateAt` let the module choose what to post. A
counterparty gains a tax number (`80012345-6`), unique per person, by which
a receipt finds its shop whatever name it is printed under; what a shop
lacks - a tax number, a usual category - it takes from the receipt that names
it, and what it has is never replaced. A city is matched however its accents
were typed, so a till's `ASUNCION` is the person's Asunción. `identity.v1`
gains `FindPerson`, for a folder named after its owner.

**A receipt is one entry with a line per item.** The account moves by what was
paid; each item is a line on the category with the item as printed; what the
till rounded off or took off the whole receipt is a line of its own. A credit
note is the same money coming back. The entry's key is the receipt's own code
(the CDC), so one receipt is one entry however many times and as whatever files
it arrives.

**What a receipt does not say is asked, never guessed.** The account is the one
the person names (or the folder the file was dropped in is named after), or the
only open cash account in the receipt's currency when it was paid in cash. The
category is the one named, or the shop's usual one. A receipt missing either
**waits**: `imports.documents` keeps the file with what it needs, the person
says it once - on the Import screen, or importing the same file again with
`--from`/`--for` - and the category said becomes the shop's usual one, so its
next receipt goes straight in. A receipt paid from an account in another
currency is booked **held** at the day's rate: what the bank took is known when
it posts, through the hold's own clearing (ADR 0010). A ledger out of reach is a
reason to wait, not to lose the document.

**A document is kept only until it is in the books.** `documents.content` is
there exactly while the document waits - a check constraint holds it - and is
dropped once the entry has the file. What stays is the trace: the hash and the
code a second arrival is matched by, what it was read as, and which entry it
became.

**The folder is the same path as an upload.** `AUSTERIS_INBOX/<email>/` holds a
person's receipts, `<email>/<account>/` those paid from that account; every
half minute each file that has stopped changing is taken in and moved to
`done/` or `refused/`. Taking a file in twice answers it with what it already
is, so a folder synchronised back in full is harmless.

**A slip is read on this machine.** Tesseract, in the image, reads the photo;
the module turns the text into the amounts and the rate, which fill the
exchange form for the person to check. Nothing is stored by reading: the photo
is kept with the exchange once it is recorded.

## Consequences

- Every importer to come - CSV statements, presets, rules - is a reader in this
  module and the same waiting path; the ledger does not learn a single format.
- The SIFEN reader is pure and tested on synthetic documents in the shape of
  version 150; a field a real shop's system writes differently is fixed there,
  and a refusal says which field.
- The image is larger by Tesseract and two languages. Without it - a build
  from source on a bare machine - slips are not read and say so; everything
  else works.
- A folder on a Linux host has to be writable by the container's user, or the
  files are read and booked but not moved; each is then answered as already in
  on every look.
