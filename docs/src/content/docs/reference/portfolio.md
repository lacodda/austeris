---
title: Portfolio
description: Trades on brokers' and exchanges' accounts, the money they move, what they add up to, what it is worth, and snapshots of it.
---

Served by the gateway under `/api/v1/portfolio`, and, like everything but
signing in, only to a caller with a session. A module: on unless the
installation switches it off (`AUSTERIS_PORTFOLIO=off`), and then its paths
answer `404` with the code `module.switched_off` and the web interface does not
offer it.

It keeps trades; the ledger keeps their money, as entries this module posts and
keeps (ADR 0015); the market knows the instruments and their prices. Every
quantity, price and amount is a string in JSON, never a number.

## Where trades happen

On your accounts of kind `brokerage` or `crypto_wallet` - a broker's, an
exchange's, a wallet - and nowhere else. The account's **balance is its money**:
dollars at a broker, or a stablecoin on an exchange, which is a currency an
account can be kept in (`USDT`, `USDC`, `FDUSD`). What it buys are **positions**.

So a stablecoin is not something to buy: an instrument whose ticker is the
currency one of your accounts is kept in is money, and a trade in it is refused
(`trade.money_not_holding`), as is one in a currency (`fx`). Money gets onto an
exchange as it gets anywhere - a transfer or an exchange in the ledger.

## What a trade moves

| Trade | The account's money | The holding | Units |
| --- | --- | --- | --- |
| `buy` | − quantity × price − fee | + the same | + quantity on the account |
| `sell` | + quantity × price − fee | − the same | − quantity on the account |
| `transfer` | nothing | nothing | − quantity on the account, + quantity − fee on the other |

The price and a buy's or a sale's fee are in the account's currency, and the
money is rounded the way that currency is - dollars to the cent, a stablecoin to
every digit. A transfer's fee is in units of the instrument, as a network takes
it.

A buy or a sale is booked in the ledger as one entry on the day it happened
where you were: the account's line, and a **holding** - the money that became
the coin, or came back from it - which no report of spending counts. The entry
is described `Buy 0.01 BTC`, signed `portfolio`, and **kept** by it: its day,
description and money change only with the trade, and it is deleted only with
the trade ([Entries a module keeps](/austeris/reference/ledger/#entries-a-module-keeps)).
Its tags, counterparty and place are yours.

## Trades

### `POST /api/v1/portfolio/trades`

```json
{
  "kind": "buy",
  "account_id": "…",
  "instrument_id": "…",
  "quantity": "0.01",
  "price": "60000",
  "fee": "1",
  "occurred_at": "2026-10-20T14:30:00-03:00",
  "note": "first one"
}
```

A transfer names `to_account_id` and no price. `occurred_at` is the instant, with
the offset of where you were: its date there is the day the money is booked
under. The answer, `201`, is the trade with `entry_id` - its money's entry - and
`money`, what it moved the account by:

```json
{ "id": "…", "kind": "buy", "quantity": "0.01", "price": "60000", "currency": "USDT",
  "fee": "1", "occurred_at": "2026-10-20T17:30:00Z", "occurred_on": "2026-10-20",
  "entry_id": "…", "money": "-601", "…": "…" }
```

**Nobody sells what they did not have at the time.** Every position a trade
touches is added up in the order its trades happened - what arrives at an
instant before what leaves - and a trade that leaves one below nothing at any
moment is refused, `409`:

```json
{ "status": 409, "code": "trade.oversold",
  "params": { "symbol": "BTC", "short": "0.01", "at": "2026-10-20T13:00:00+00:00" } }
```

The same holds for a trade changed or deleted - deleting a buy that a later
sale sold from is refused - and for two sales sent at once: positions are added
up one write at a time, so the second finds what the first left.

Other refusals, `400`, each with its code:

| Code | Why |
| --- | --- |
| `trade.not_a_holding_account` | the account is not a broker's or a wallet's |
| `trade.closed_account` | the account is closed |
| `trade.unknown_account`, `trade.unknown_instrument` | no such account of yours, no such instrument |
| `trade.money_not_holding` | the instrument is money here |
| `trade.fee_too_large` | the fee is all of it: nothing arrives, or a sale brings nothing in |
| `trade.nothing_moved` | the money rounds to nothing in the account's currency |
| `trade.refused_by_ledger` | the ledger refused the money, saying why |

When the ledger or the market does not answer, `503` with
`portfolio.ledger_unavailable` or `portfolio.market_unavailable`, and nothing is
recorded on either side.

### `GET /api/v1/portfolio/trades`

The latest first. Takes `?account=` (on it, or arriving at it), `?instrument=`,
`?limit=` (100 by default, 500 at most) and `?offset=`.

### `GET /api/v1/portfolio/trades/{id}`

### `PUT /api/v1/portfolio/trades/{id}`

Says what a trade is now - every field, as recording it does - and restates its
money in the same entry. A buy said to be a transfer takes its entry back; a
transfer said to be a buy books one.

### `DELETE /api/v1/portfolio/trades/{id}`

The trade, and its money out of the books. Its history stays.

### `GET /api/v1/portfolio/trades/{id}/history`

Who recorded, changed and deleted it, and what it was before and after each,
oldest first - kept by the database, as an entry's history is.

## Positions

### `GET /api/v1/portfolio/positions`

Units of each instrument on each account, every position not empty. Takes
`?on=`, a day: the trades of that day and before.

```json
[{ "account_id": "…", "instrument_id": "…", "quantity": "0.006" }]
```

## Value

### `GET /api/v1/portfolio/value`

Each broker's and wallet's account, its money and its positions, what each is
worth and what it all adds up to, in one currency. Takes `?currency=` (`USD` by
default) and `?on=`, a day past.

```json
{
  "on": "2026-10-26", "at": "2026-10-26T15:00:00Z", "currency": "USD",
  "accounts": [{
    "account_id": "…", "name": "Exchange", "total": "768.851",
    "holdings": [
      { "kind": "cash", "symbol": "USDT", "quantity": "149", "price": "0.999", "value": "148.851", "stale": false },
      { "kind": "position", "instrument_id": "…", "symbol": "BTC", "name": "Bitcoin",
        "quantity": "0.01", "price": "62000", "value": "620", "priced_at": "…", "stale": false },
      { "kind": "position", "instrument_id": "…", "symbol": "ETH", "name": "Ethereum", "quantity": "0.1", "stale": false }
    ]
  }],
  "total": "768.851", "unvalued": ["ETH"], "stale": false
}
```

- **Money** is the account's balance in the ledger, at the ledger's rate for
  the day ([Rates](/austeris/reference/ledger/#rates)) - a stablecoin too: its
  rate is typed or published, never assumed to be one.
- **A position** is at the market's latest price in that currency. When the
  market has none in it - a source quotes a coin in dollars and you read
  guaraníes - it is the dollar price, then the euro price, at the ledger's rate
  for the day.
- **What has no price** is listed with its quantity and named in `unvalued`, and
  is not in the total. A total that silently drops it looks like an answer.
- **`stale`** marks a price older than its kind stays current for - two hours
  for a coin - or a rate older than a long weekend.

**A day past** is the trades of that day and before, the money at its end, and
the prices last observed by then. The books know days and not moments - an
entry has no time - so a day is what can be shown truthfully.

## Snapshots

### `POST /api/v1/portfolio/snapshots`

```json
{ "currency": "USD", "note": "before the summer", "on": "2026-06-30" }
```

Keeps a valuation as it is - what each account held, at what price, worth how
much - of today, or of a day past (`on`). It reads the same after a trade of its
time is corrected or a price revised. `201` with the snapshot: the valuation,
with `id`, `note` and `taken_at`.

### `GET /api/v1/portfolio/snapshots`

The latest day first: `id`, `on`, `currency`, `note`, `taken_at`, `total`, and
whether something had no price (`incomplete`) or a stale one (`stale`).

### `GET /api/v1/portfolio/snapshots/{id}`

The snapshot beside the portfolio now, in the snapshot's currency - or beside
another snapshot, `?against=`. `differences` has every line of either, with
what it was and is:

```json
{ "account_id": "…", "account_name": "Exchange", "kind": "position", "symbol": "BTC",
  "quantity_then": "0.01", "quantity_now": "0.015", "value_then": "600", "value_now": "960" }
```

A line held then and not now has `quantity_now` zero; one bought since has
`quantity_then` zero. The 2025 tracker's snapshots said only how many more were
held; these say what it is worth as well.

### `DELETE /api/v1/portfolio/snapshots/{id}`

For good. No trade is touched.

## Prices by hand

Without a CoinMarketCap key the market has no coin prices, and a position has no
value until one is said: `POST /api/v1/market/instruments/{id}/prices`
([Prices and rates](/austeris/reference/market/#post-apiv1marketinstrumentsidprices)),
or **Price it** on the Instruments tab.
