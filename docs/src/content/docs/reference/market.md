---
title: Instruments and prices
description: What can be priced, where prices come from, and how to ask for one.
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

An instrument with no price is **absent from the answer**, not an error: one
unpriced instrument must not cost a whole batch.

### `GET /api/v1/market/prices/{id}/history`

Every observation in a window, oldest first. Takes `?currency=`, `?from=` and
`?to=` as RFC 3339 timestamps; the last thirty days by default. A window that
ends before it starts is refused rather than answered empty — an empty list
reads as "no prices" rather than "bad question".

### `POST /api/v1/market/prices/refresh`

Asks every available source for the instruments bound to it.

```json
{ "recorded": 42, "sources": ["coinmarketcap"], "failed": [] }
```

Sources are independent: one failing costs its own instruments' prices and
nothing else, and says so in `failed` rather than reporting a clean run.

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
service Market {
  rpc GetPrices(GetPricesRequest) returns (GetPricesResponse);
  rpc GetPriceAt(GetPriceAtRequest) returns (GetPriceAtResponse);
}
```

`GetPrices` takes several instrument ids at once, because the caller that wants
one price usually wants forty. `GetPriceAt` takes Unix seconds and applies the
same "as of" rule as the REST surface.

Prices cross this wire as strings too: protobuf has no decimal type, and a
price that travels as a double is not the price that was recorded.
