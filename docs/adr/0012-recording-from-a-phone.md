# 12. Recording from a phone: a second press, templates, previews and coded errors

Date: 2026-10-12

## Status

Accepted. Builds on [0007](0007-entries-balance-by-construction.md) and
[0011](0011-the-web-interface.md).

## Context

From 0.11 the phone is where money is recorded, at the till, with a thumb. Four
questions come with it that outlive the screen that raised them: what stops a
double tap from booking twice, what a button that records a payment again is
made of, how a typed line is shown for what it means before it is recorded,
and how the server's refusals reach a screen in the reader's language.

## Decision

**A second press is refused by the ledger, not filtered by a client.** An entry
without an idempotency key - one a person recorded - is refused with `409` and
the code `entry.possible_duplicate` when the same person recorded the same entry
in the last sixty seconds: the same day, description and counterparty, and the
same lines as a multiset of side, account or category, amount and currency.
`allow_duplicate` records it anyway. The check runs inside the posting
transaction after `pg_advisory_xact_lock` on the person, so two presses that
arrive together are checked one after the other and cannot both find nothing.

A client-made idempotency key was the alternative and was not taken: it covers a
retried request, not a second press of the same button - two presses are two
requests with two keys - nor a press again after nothing seemed to happen, nor
the command line. One rule in the ledger covers every surface that records:
an entry, a typed line, an exchange, a template, an entry recorded again.
Modules keep their keys, and the check skips an entry that has one.

**A template is kept by what it points at.** `ledger.templates` holds an
account, a category, an optional counterparty and place, tags through
`template_tags`, an amount or `NULL` for "ask each time", the currency the amount
is in, and the person's order. Every reference names the owner, as an entry's
do. A stored line - `45000 food @cafe` - was the alternative: smaller, and the
same grammar, but it stops meaning anything the day `food` is renamed, which in
the first weeks of real books is when categories are renamed most.

A template is made from an entry, never filled in as a form, and only from one
payment - one account, one category, and the conversion between them; reading an
entry that way is `paid::of`, pure, and the same function decides what an entry
recorded again is converted from.

**Recording again copies, except where a copy would lie.** An entry in one
currency is its lines, and they are copied as they are - a split receipt and a
transfer included. A payment charged in another currency keeps what it cost
there and is converted again at the day's rate; an exchange between two
accounts is refused. The copy is held when the original went through a hold.

**Working out a payment is apart from writing it.** A typed line, a template
and a converted entry recorded again all become one `Payment` - an account, a
category, an amount in a currency, the rate when one was stated - which is
priced against the rates without writing anything, and only then turned into
lines, which may create the fee category. The preview of a typed line stops
after pricing: it answers what recording would record, refused for the same
reasons with the same codes, and leaves no counterparty, tag or category behind.

**A refusal a client acts on carries a code.** `ErrorBody` gains `code` - a
stable `area.what` - and `params`, the facts the message was written from. The
codes are declared together with the list of them in `austeris-common`, so one
cannot be answered without being listed, and a test of the gateway's crate holds
the web interface's locale files to a sentence for every listed code, and to no
sentence for a code nobody answers with. The message stays, for the command line,
for logs, and for a code a client does not know yet. Every refusal of a typed
line has its code from 0.11; the rest of the API is given codes as screens
need them.

**The interface installs.** A web manifest opens the installed app on
`/record`, full screen, with the plate's colour; the icons are rendered from
the brand's SVGs by the same script as the other rasters, the maskable one on
the plate inside the safe zone.

## Consequences

A person really buying the same thing twice within a minute is asked once. The
lock serialises one person's typed entries, which a household cannot feel.

`entries` gains a partial index on `(owner_id, created_at)` for entries without
a key; the check reads the last minute of it.

Rolling the schema back past 0.11 is refused while any template exists.

A screen decides by `code` and says the sentence in its own language; a client
that ignores codes still has `message`, so nothing older breaks.
