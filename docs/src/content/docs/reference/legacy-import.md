---
title: Moving from the 2025 tracker
description: Bringing the crypto tracker austeris grew out of - its wallets, buys and sells, prices and snapshots - into the books, and reading where the two part.
---

Before austeris kept books it was a crypto tracker (2025): buys and sells of a
coin on a wallet, at a price, with a fee, the prices it fetched from
CoinMarketCap, and snapshots of what was held, in a PostgreSQL database of its
own. `austeris import legacy` brings all of it in, and says where the books and
the tracker disagree.

```sh
pg_dump --dbname=postgres://tracker@localhost:5432/austeris > legacy.sql
austeris login --email you@example.com
austeris import legacy legacy.sql --dry-run
austeris import legacy legacy.sql
```

## The dump

A plain `pg_dump` of the tracker's database - the SQL script it writes by
default, gzipped or not. Only its `COPY` blocks are read; nothing in the file is
executed, and no database is needed to read it. An archive (`pg_dump -Fc`, `-Ft`
or `-Fd`, and pgAdmin's backup by default) is refused with the one command that
turns it into a script, `pg_restore --file legacy.sql <archive>`; a script made
with `--inserts` is refused too. A file Windows PowerShell 5.1 redirected - in
UTF-16 - is read as it is.

## What it becomes

| The tracker | The books |
| --- | --- |
| a wallet | an account of kind `crypto_wallet`, kept in USDT, with the wallet's name |
| a buy or a sell of a coin | a trade on that account, at its price, with its fee - and its money booked ([Portfolio](/austeris/reference/portfolio/)) |
| USDT | the account's money, not a coin |
| a coin | an instrument, bound to CoinMarketCap by the id the tracker knew it by |
| a price | a price of the market, under the source `austeris-2025` |
| a USDT price | the ledger's rate of USDT in dollars for its day |
| a snapshot | a snapshot of its day, worked out again from the trades |

**The days.** The tracker's moments have no zone; its server ran in UTC. Each
trade is booked under the day it fell on where you were: this machine's time
zone, or `--offset -03:00`.

**The floats.** The tracker kept amounts as binary floats, and a sum like
`2000 - 1701.7` came out as `298.29999999999995`. A float holds fifteen
significant digits of a decimal and no more, so each amount is read as its first
fifteen: `298.3`.

**The money.** The tracker kept coins, not money: a Bitcoin buy did not say what
paid for it, and USDT was one more coin. The books keep the money a trade moves,
so the USDT the tracker recorded is read in two ways:

- A USDT row that was a trade's money - the other way, recorded within the hour,
  for nearly what the trade cost (2%) - is dropped. The books book a trade's
  money from the trade.
- Every other USDT row came into the wallet, or left it, from outside what the
  tracker saw. The books cannot date money from outside them, so it goes into
  the account's **opening balance**, with whatever more the trades needed to have
  been paid for, which the tracker never saw arrive: the least that keeps the
  account's money from ever going below nothing.

**What the books cannot say** is left out and named, with why: a coin received
for nothing (a reward, an airdrop - there is no trade for that yet), a sale of
more than the trades before it bought, a number that is not one.

**An account already there** - of the wallet's name, a broker's or a wallet's
account in USDT - is used as it is, its opening balance untouched; the report
says what the tracker's records asked for. One of that name of another kind
leaves the wallet to `Binance (2025)`.

## Running it twice

Every trade and snapshot is recorded under the name of the row it came from
(`austeris-2025:transaction:17`), prices under their moment and source, rates
under their day: running the import again changes nothing, and a run that
stopped half way is finished by running it again. A trade deleted in the books
since is brought back by a second run.

## The report

It says what each wallet opened with and why, which trades were left out and
why, which USDT rows were a trade's money, and then sets the tracker beside the
books - each snapshot's day, and the end of the records wallet by wallet - with
what each was worth in dollars:

```text
  2025-04-15 - the tracker's #4 of 2025-04-15 12:00 UTC
                                       the tracker             the books
    BTC                                      0.025                 0.025
    ETH                                        0.6                   0.6
    SOL                                          6                     0  - differs
    USDT                                    717.88                739.26  - differs
    worth, USD                             4500.13                3768.5  - less by 731.62
```

A difference is the import saying what it could not keep, not an error: the
Solana received for nothing that the books leave out, USDT the tracker recorded
as spent that the books took from a trade, money the tracker never saw. The
tracker's side is worth what its own prices said at the end of that day; the
books', what the books say. `--dry-run` writes nothing and needs no sign-in, and
sets the tracker beside what the books will hold.
