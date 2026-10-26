# 15. The portfolio, and the money it moves

Date: 2026-10-26

## Status

Accepted. Builds on [0001](0001-microservices-with-guardrails.md),
[0007](0007-entries-balance-by-construction.md),
[0009](0009-conversions-and-official-rates.md) and
[0013](0013-the-memory-of-an-entry.md).

## Context

The 2025 tracker this project grew out of kept crypto trades - buys and sells
of a coin on a wallet, at a price, with a fee - added them up into holdings,
valued them at CoinMarketCap's prices and kept snapshots of them to compare
with later. It kept them on their own: the money a purchase cost was nowhere
else, so the books and the coins never met.

In the new model the books are the ledger's. A trade is money becoming
something that is not money - a coin, a share - on an account at a broker or an
exchange, and that money has to be in the books. Three things did not fit:

- **Where the money goes.** A line names an account, a category or a
  conversion (ADR 0009). A purchase is none of them: the coin is no currency of
  the person's, it was not spent, and nothing arrived in another currency the
  ledger keeps.
- **Who decides what the money was.** The trade does - its quantity, price and
  fee - and the entry is derived from it. An entry a person could delete or
  move to another day would tell a different story from the trade it came
  from; editing or deleting a trade has to restate or take back its entry.
- **What an exchange's money is.** On an exchange the cash is a stablecoin -
  USDT - and the person thinks of it as dollars waiting to be spent, not as a
  position whose price moves. A currency code had three letters.

## Decision

**A module of its own, `portfolio`**, with its schema and prefix
(`/api/v1/portfolio`) and its switch from birth (`AUSTERIS_PORTFOLIO=off`). It
keeps trades - `buy`, `sell`, `transfer` - on the person's accounts of kind
`brokerage` or `crypto_wallet`, and nothing else: a position is what the trades
add up to, worked out when asked and never stored beside them.

**A fourth side of a line, `holding`**: money that became something a module
holds, or came back from one. It names nothing, like a conversion; which coin it
became is the module's to know. A buy of 0.01 BTC at 60 000 USDT with a fee of 1
is `-601 USDT` on the exchange account and `+601 USDT` on the holding; a sale
is the reverse, its proceeds less its fee. Every currency still sums to zero,
the account's balance is its cash, and no report of spending counts a holding.
The fee is part of what the coin cost - which is how a cost basis counts it -
until fees get a category of their own.

**An entry a module keeps.** `PostEntry` gains `kept`: the module that posts
it, under its idempotency key, alone restates it (`RestateEntry`: its day,
description and lines) or takes it back (`WithdrawEntry`). The schema refuses
anyone else - a person through the API, or another module - a change to a
kept entry's day, description or lines, and its deletion (SQLSTATE `AUK01`,
answered as `409 entry.kept` naming the module). Its tags, counterparty, place
and files stay the person's. A holding line belongs only to a kept entry, so a
person cannot type money into a coin no trade knows about. The trash leaves
out what a module took back: only the module could bring it back. The
mechanism is the ledger's, not the portfolio's: a loan's schedule and a salary
run will keep their entries the same way.

**The trade writes first, and commits once the ledger has answered.** The
trade is written in the portfolio's transaction - where the schema checks it
- then its money is posted, restated or withdrawn, and only then does the
transaction commit. A ledger that does not answer leaves nothing recorded on
either side. Should the commit itself fail after the ledger answered, the
ledger is asked to undo what it did, and if even that fails the log says the
two disagree.

**No position goes below nothing, at any moment.** A trigger adds up each
position a write touches in the order its trades happened - what arrives at an
instant before what leaves - and refuses one that dips below zero (`AUP01`,
answered as `409 trade.oversold` with how many short and when). Positions are
locked while they are added up, so two sales of the same coins at once are one
sale. A transfer's fee is in units of the instrument, as a network takes it; a
buy's and a sale's are in the account's currency, as its price is.

**A trade remembers who made and changed it**, by the same trigger-written
revisions as an entry (ADR 0013), in the portfolio's own schema.

**A stablecoin is a currency.** A currency code is three to five letters, so an
account can be kept in USDT, USDC or FDUSD. An exchange account in USDT is
where its cash is; what it buys are positions. The value of an account is its
cash at the ledger's rate (ADR 0009) plus its positions at the market's
prices. A stablecoin's rate is a rate like any other - typed, or published by a
source - never assumed to be one.

**Value in any currency the books can reach.** A position is priced in the
currency asked for; when the market has no price in it - a source quotes a
coin in dollars, the person reads guaranies - in dollars, then euros, at the
ledger's rate for the day. What has no price or rate is listed with its
quantity and named, never silently left out of a total.

**Snapshots are of a day.** An entry has a day and no time, so the money beside
a position is known only at a day's end. A snapshot - and a valuation of the
past - is the trades of a day and before, the money at its end, and the prices
observed by then; taken today, the latest. It keeps its lines as they were,
with what each was worth, so it reads the same after a trade of its time is
corrected, and it is compared line by line with now or with another snapshot.

**A price can be said by hand.** `POST /market/instruments/{id}/prices`
records one under the source `manual`; the newest price is the latest whoever
said it, and it goes stale on its kind's calendar as a source's would. Without
a CoinMarketCap key it is the only way a coin has a value.

## Alternatives

**An account per position, in the instrument as a currency.** Buying 0.01 BTC
would be a conversion from USDT into BTC on an account kept in BTC. The quantity
would live twice - in the ledger and in the trades - and the ledger would
pretend AAPL is a currency, with a rate to every other.

**The purchase as an expense under an "Investments" category.** No new side,
and the money balances. But every report of spending would count a purchase of
shares as spending, and a sale as income.

**Trades immutable, corrected by a reversing entry.** Nothing changes in the
ledger's contract. But a corrected trade leaves two entries for one movement,
and a deleted one leaves a pair that cancels: books that are right in total and
wrong to read.

**Positions stored, updated with each trade.** Faster to read. But a quantity
kept beside the trades is a second copy of the same truth, and the first edit
of an old trade parts them.

## Consequences

- The ledger's contract grows: `Line.holding`, `PostEntryRequest.kept`,
  `RestateEntry`, `WithdrawEntry`; the market's gains `GetInstruments`.
- An entry's API carries `kept`, and the web interface offers no deletion,
  repetition or day of its own for one.
- A rollback of the ledger past this release is refused while an entry is kept
  or a holding exists, in the books or in their history; the portfolio's while a
  trade or a snapshot exists.
- Switching the portfolio off leaves its entries kept: they are still changed
  only through it, once it is back.
- Lots, cost basis and realised gains (v0.16) are worked out from the trades;
  restating every kept entry is how fees will move to a category of their own.
