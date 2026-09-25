---
title: Prices and rates
description: What can be priced, where prices and official exchange rates come from, and how old they are.
---

Served by the gateway under `/api/v1/market`, and, like everything but signing
in, only to a caller with a session. This service answers one kind of
question — what an instrument is worth, now or at a past instant. Who holds it
and what they paid is the portfolio's business, and this service never learns
either.

Every price is a **string** in JSON, never a number: a client parsing a JSON
number gets an IEEE double and has lost the value before it renders it. The
trailing zeros are the column's scale, `NUMERIC(38, 18)`, reported rather than
trimmed.

## Where prices come from

| Source | Prices | Needs |
| --- | --- | --- |
| `coinmarketcap` | crypto, in USD | `AUSTERIS_CMC_API_KEY`; switched off without it |
| `bcp` | currencies in guaranies — Banco Central del Paraguay's daily reference rate | nothing |
| `cbr` | currencies in roubles — the Bank of Russia's official daily rate | nothing |

The service refreshes every source that is on **once an hour**, starting a few
seconds after it comes up; nobody has to remember to fetch this morning's rates.
Each currency a central bank publishes becomes an `fx` instrument named by its
code the first time it is seen, priced in the bank's own currency and observed
at the start of the day the bank set it for.

Official rates are also handed to the ledger, which keeps the rates it values
money at ([Rates](/austeris/reference/ledger/#rates)). The ledger never asks for
them — the core knows no module — so the market pushes each table it records,
and the ledger keeps any rate it already had for that day.

Both banks are asked over HTTPS, at `www.bcp.gov.py` and `www.cbr.ru`; an
installation behind a firewall that blocks them keeps working, with rates that
say how old they are.

## Instruments

### `GET /api/v1/market/instruments`

Everything that can be priced, ordered by kind and symbol.

### `POST /api/v1/market/instruments`

```json
{ "kind": "crypto", "symbol": "BTC", "name": "Bitcoin", "decimals": 8 }
```

`kind` is one of `crypto`, `stock`, `bond`, `fund`, `fx`, `manual`. Creating an
instrument that already exists updates its name and returns the same id, so a
sync run twice does not produce two Bitcoins.

### `POST /api/v1/market/instruments/sync`

Imports a source's catalogue, binding each entry to that source. Takes
`?limit=` (100 by default). Answers `503` when the source is switched off —
which is what a missing `AUSTERIS_CMC_API_KEY` means.

### `POST /api/v1/market/instruments/{id}/sources`

Points a source at an instrument.

```json
{ "source": "coinmarketcap", "external_id": "1", "priority": 10 }
```

`priority` is a rank: **lower wins**. An instrument with several sources falls
back down the list, which is the answer to a source going down and prices
simply stopping — what happened in 2025, with nothing saying so.

## Prices

### `GET /api/v1/market/prices`

The latest price of each instrument. Takes `?instruments=` (comma-separated
ids; all of them when omitted) and `?currency=` (`USD` by default).

```json
[{ "instrument_id": "…", "quote_currency": "PYG", "observed_at": "2026-09-24T00:00:00Z",
   "price": "5900.280000000000000000", "source": "bcp", "stale": false }]
```

An instrument with no price is **absent from the answer**, not an error: one
unpriced instrument must not cost a whole batch.

A price older than its kind stays current for is still the answer — it is the
last one known — and says `"stale": true`:

| Kind | Stale after |
| --- | --- |
| `crypto` | two hours: it trades every hour of every day |
| `fx`, `stock`, `bond`, `fund` | four days: priced on business days, so a weekend with a holiday on either side is still current |
| `manual` | never: it is as current as whoever priced it says |

### `GET /api/v1/market/prices/{id}/history`

Every observation in a window, oldest first. Takes `?currency=`, `?from=` and
`?to=` as RFC 3339 timestamps; the last thirty days by default. A window that
ends before it starts is refused rather than answered empty — an empty list
reads as "no prices" rather than "bad question".

### `POST /api/v1/market/prices/refresh`

What the hourly refresh does, now: asks every available source, records what it
said, and hands official rates on to the ledger.

```json
{ "recorded": 57, "sources": ["bcp", "cbr"], "rates_handed_on": 55,
  "failed": [{ "source": "coinmarketcap", "error": "CoinMarketCap answered 429 Too Many Requests" }] }
```

Sources are independent: one failing costs its own prices and nothing else, and
is named in `failed` with what went wrong rather than reported as a clean run. A
refresh that recorded the rates but could not hand them to the ledger names
`ledger` there.

`?on=YYYY-MM-DD` asks for a past day instead — the central banks answer for any
date, so an exchange recorded for last month can be measured against that day's
rate. Sources without history (`coinmarketcap`) are named in `skipped`. A day
that has not happened yet is refused.

### `GET /api/v1/market/sources`

Every source, whether it is on, and how old what it last said is:

```json
[
  { "name": "coinmarketcap", "prices": "crypto", "available": false,
    "off_because": "AUSTERIS_CMC_API_KEY is not set",
    "last_observed_at": null, "stale": true },
  { "name": "bcp", "prices": "fx", "available": true,
    "last_observed_at": "2026-09-24T00:00:00Z", "stale": false }
]
```

The answer to *why is this price from last Tuesday*: a source switched off for
want of a key, or one that has stopped answering, shows here with the time of
its last observation.

## Which price answers

Two rules, applied in this order:

1. **Recency.** The newest observation at or before the instant asked about. A
   price is a fact about a moment that has already happened, so a later one
   answers a different question — and valuing a trade with a price from after
   it is how a portfolio ends up worth what it never was.
2. **Priority.** Among observations at the same instant, the best-ranked source.

A newer price from a fallback source therefore beats a stale one from the
preferred source, which is the whole point of having a fallback.

Removing a source binding stops the source being asked; it does not erase what
it already told us. History outlives the binding.

## Between services

```proto
service MarketService {
  rpc GetPrices(GetPricesRequest) returns (GetPricesResponse);
  rpc GetPriceAt(GetPriceAtRequest) returns (GetPriceAtResponse);
}
```

`GetPrices` takes several instrument ids at once, because the caller that wants
one price usually wants forty. `GetPriceAt` takes Unix seconds and applies the
same "as of" rule as the REST surface. Each price carries `stale`, measured
against the instant asked about: last week's price is current for a question
about last week.

Prices cross this wire as strings too: protobuf has no decimal type, and a
price that travels as a double is not the price that was recorded.
