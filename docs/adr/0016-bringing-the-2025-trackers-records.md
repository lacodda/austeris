# 16. Bringing the 2025 tracker's records

Date: 2026-10-31

## Status

Accepted. Builds on [0001](0001-microservices-with-guardrails.md),
[0009](0009-conversions-and-official-rates.md) and
[0015](0015-the-portfolio-and-the-money-it-moves.md).

## Context

The crypto tracker of 2025 - the code under the `legacy-2025` tag - kept a
person's coins in a PostgreSQL database of its own: `assets`, `wallets`,
`transactions` (a buy or a sell of an asset on a wallet, at a price in dollars,
with a fee), `portfolio_snapshots` (what was held, as a JSON list) and
`asset_prices` (CoinMarketCap's dollar prices it fetched). Amounts were binary
floats, moments had no zone, and nothing said what money paid for a coin: USDT
was one more coin.

The books are another shape. A trade moves money on its account, as an entry
the portfolio keeps (ADR 0015); USDT on an exchange is that account's money; a
snapshot is a valuation of a day. Bringing the records over is not a copy, and
three things had to be decided: where the import runs, what makes a second run
harmless, and what USDT and money the tracker never saw become.

## Decision

**A client of the public API, reading a plain dump.** `austeris import legacy`
reads the `COPY` blocks of a plain `pg_dump` script - nothing in it is executed,
and no database is needed - works out what each row becomes, and writes it
through the gateway as the signed-in person: accounts through the ledger,
instruments and prices through the market, trades and snapshots through the
portfolio, with every rule each applies. An archive is refused with the
`pg_restore --file` command that makes it a script; UTF-16, a byte-order mark,
CRLF and gzip are undone. `--dry-run` works it all out and writes nothing.

**A record names itself, so a second run is the same records.** A trade and a
snapshot take an `idempotency_key` - unique per person, set when the record is
made, kept when it is changed - and recording under a key already used answers
the record made under it (`200`), whatever the second request says. Two
requests racing under one key are one record: the unique index makes the second
wait, and it answers the first. The import names each trade
`austeris-2025:transaction:<id>` and each snapshot
`austeris-2025:snapshot:<id>`; prices are corrected rather than added under their
instrument, moment and source; rates under their day.

**A history of prices is brought under the name of where it came from.**
`POST /market/prices` records up to five thousand observations at once, all or
none, under a source of the caller's naming - `austeris-2025` - that may not be
the name of a source the installation asks itself. They were CoinMarketCap's
quotes, but this installation did not hear them; recorded under its name they
would make the source's history and freshness say what it never said. A USDT
price is the ledger's rate of USDT in dollars for its day, its last of the day.

**A float is fifteen digits.** Every amount is read as its first fifteen
significant digits - all a binary float holds of a decimal - so `298.29999999999995`,
a sum the tracker computed, is the `298.3` it was.

**USDT is money, and money from outside the books opens the account.** Each
wallet becomes an account of kind `crypto_wallet` in USDT, and each buy or sell
of another coin a trade on it. A USDT row that is the money of such a trade -
the other way, on the same wallet, within an hour, within 2% of what the trade
moved - is dropped: the books work a trade's money out from the trade. Every
other USDT row is money from outside what the tracker saw, which the books
cannot date - the ledger has no side for money arriving from outside the books
- so their sum is the account's opening balance. To it is added what the
trades needed to have been paid for, replaying the wallet's money in time: the
least that keeps it from ever going below nothing.

**What the books cannot say is left out and named.** A coin received for
nothing, a sale of more than the trades before it bought (found by replaying
each position as the portfolio's trigger adds it up), a row that does not read.

**Snapshots are worked out again, and set beside the tracker's.** A tracker
snapshot becomes a snapshot of its day - the day's last when it took several -
valued in dollars from the trades the books now hold. The report sets each, and
the end of the records wallet by wallet, beside what the tracker said and what
it was worth at its own prices: the differences are what the import could not
keep, said rather than hidden.

## Alternatives

**A module that reads the dump on the server.** One upload instead of hundreds
of calls - but a second path into three services' schemas for a job done once,
with the account, instrument and trade rules either repeated or called over
contracts the portfolio does not serve.

**Reading the tracker's live database.** No dump to make, but the tracker's
PostgreSQL has to be running and reachable, where a dump is a file the person
already knows how to make and can keep.

**Every USDT row as a dated deposit or withdrawal.** Truer to the day, but the
ledger has no side for money from outside the books: such an entry would be
income in every report of what was earned, or half of an exchange with no other
half.

**Snapshots kept as the tracker kept them.** Its lists name no account and no
price, and a snapshot that disagrees with the trades beneath it is a second truth
about the same coins. The tracker's own lists stay in the report.

**Finding what was imported instead of naming it.** Matching a trade by its
account, instrument and moment, as the demo does, misses one corrected since,
and a snapshot has nothing to match on.

## Consequences

- The portfolio's schema gains `idempotency_key` on trades and snapshots; a
  rollback is refused while one is set.
- The market's contract gains `POST /market/prices`; the prices it records are
  read like any other.
- A trade deleted after an import is brought back by running it again: a key
  names a record that exists, not one that once did.
- An opening balance stands for money the books cannot date, so a snapshot of a
  day before a deposit shows it already there; the report says so in what it
  sets beside the tracker.
- A coin received for nothing waits for a trade kind of its own.
