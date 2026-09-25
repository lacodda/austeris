---
title: Accounts, categories and entries
description: The bookkeeping core - what it stores, and how to record a movement.
---

Served by the gateway under `/api/v1/ledger`, and, like everything but signing
in, only to a caller with a session. Everything here belongs to the person whose
session it is; there is no path that reads across people.

Every amount is a **string** in JSON, never a number: a client parsing a JSON
number gets an IEEE double and has lost the value before it renders it. The
trailing zeros on a stored amount are the column's scale, `NUMERIC(38, 18)`,
reported rather than trimmed.

Why an entry has sides, and what that buys, is in
[Why an entry has sides](/austeris/concepts/double-entry/).

## Accounts

Where money sits. An account is in **one** currency; a second currency means a
second account, because an account holding two has no balance anyone can state.

### `GET /api/v1/ledger/accounts`

Every account, open ones first.

### `POST /api/v1/ledger/accounts`

```json
{ "kind": "cash", "name": "Wallet", "currency": "PYG", "opening_balance": "500000" }
```

`kind` is one of `cash`, `bank`, `card`, `brokerage`, `crypto_wallet`, `deposit`,
`loan`. The kinds differ in what they mean to a person, not in how they are
posted to — a card and a loan are both a balance that lines move.

`opening_balance` is what was in the account before austeris knew about it, and
defaults to zero. It is a fact about the account rather than an entry: booking it
would put money in reports of what was earned and spent.

Answers `409` when the person already has an account by that name.

### `POST /api/v1/ledger/accounts/{id}/close`

```json
{ "closed": true }
```

Closed, never deleted: its entries are still true, and a report of last year must
not change because an account was shut this morning. `{"closed": false}` reopens
it.

## Categories

What money is earned from or spent on — in bookkeeping terms, the income and
expense accounts. They form a tree.

### `POST /api/v1/ledger/categories`

```json
{ "flow": "expense", "name": "food", "parent_id": null }
```

`flow` is `income` or `expense`. Omitting `parent_id` makes it a root.

Siblings must be distinct, but the same leaf name under two parents is fine:
`living/taxi` and `travel/taxi` are two different things to spend on. When a bare
name means both, the one-line form asks which rather than guessing.

## Entries

### `POST /api/v1/ledger/entries`

```json
{
  "occurred_on": "2026-09-17",
  "description": "supermarket",
  "lines": [
    { "account_id": "…", "amount": "-50000", "currency": "PYG" },
    { "category_id": "…", "amount": "30000", "currency": "PYG", "note": "groceries" },
    { "category_id": "…", "amount": "20000", "currency": "PYG", "note": "bus fare" }
  ]
}
```

At least two lines, each naming exactly one of an account, a category, or
`"conversion": true`. Amounts are signed: negative leaves, positive arrives.
`occurred_on` is the day the money moved — not the instant it was recorded — and
defaults to today.

A line on an account is in the account's currency; money in another currency
reaches it through a **conversion**. An entry's conversion lines take one
currency in and give another out — how is in
[Money that changes currency](/austeris/concepts/double-entry/#money-that-changes-currency).
Every line comes back with its `side`: `account`, `category` or `conversion`.

The lines must sum to zero in every currency they touch. They are checked here
and again at the database's commit; an entry that does not balance is refused
with the currency and the gap named:

```json
{ "status": 400, "error": "Bad Request",
  "message": "the entry does not balance in PYG: the lines sum to -100" }
```

### `POST /api/v1/ledger/entries/quick`

One typed line, which is the fastest way there is to record an expense:

```json
{ "text": "45000 food lunch at the corner" }
```

The shape is `[+]<amount> [<currency>] <category> [from|to <account>] [@<rate>] [note…]`:

| Typed | Means |
| --- | --- |
| `45000 food` | 45 000 spent on `food`, from the only open account |
| `45000 food lunch at the corner` | the same, with everything after the category as the note |
| `45000 food from cash lunch` | …from the account called `cash`; `from` and `to` both name one |
| `+2500000 salary september` | money coming **in**: `+` is the only mark that turns a line around |
| `25000 travel/taxi` | a path, when a bare name means more than one category |
| `1,500.50 food` / `1.500,50 food` | the same amount; thousands separators and either decimal mark |
| `10 usd subscriptions from card` | 10 USD charged to a guarani card, converted at the day's rate |
| `10 usd subscriptions @5965 from card` | …at the rate the bank actually charged |

`-45000` is **not** accepted as the opposite of `+`. An expense is what a bare
line already means, and a second spelling of the common case is how a `-` typed
out of habit comes to record income.

What the parser cannot know is resolved against your own books, and refused
rather than guessed:

- a category you do not have → `404`, naming it;
- a name meaning two categories → `409`, listing them;
- two open accounts and no `from` → `400`, listing them;
- `45000 salary`, an expense filed under an income category → `400`.

The same parser serves `austeris add` and, later, the phone screen: a sentence
means the same thing wherever it is typed.

An amount in a currency other than the account's is **converted**. The category
keeps the amount as it was charged — a 10 USD subscription is 10 USD in any
report kept in dollars — and the account moves by what that was in its own
currency, rounded to the places that currency has (whole guaranies, cents). The
rate is the one written after `@`, or the day's rate when there is none; with no
rate known for the day at all, the line is refused and asks for one. A rate that
differs from the day's is measured against it, and the difference is filed as an
exchange fee, exactly as for an [exchange](#exchanges).

The answer is the entry and, when money changed currency, what converting it
did:

```json
{
  "entry": { "id": "…", "occurred_on": "2026-09-25", "description": "streaming", "lines": ["…"] },
  "conversion": {
    "given": { "amount": "59650", "currency": "PYG" },
    "got": { "amount": "10", "currency": "USD" },
    "deal": { "base": "USD", "quote": "PYG", "rate": "5965" },
    "reference": { "base_currency": "USD", "quote_currency": "PYG", "rate": "5900.28",
                   "on_date": "2026-09-24", "source": "bcp", "age_days": 1, "stale": false },
    "fee": { "amount": "647", "currency": "PYG" }
  }
}
```

### `GET /api/v1/ledger/entries`

Newest first. Takes `?from=`, `?to=`, `?account=`, `?category=`, `?limit=`
(50 by default, 200 at most) and `?offset=`.

### `DELETE /api/v1/ledger/entries/{id}`

A whole entry with its lines, never one line: half an entry is money from
nowhere.

## Exchanges

### `POST /api/v1/ledger/exchanges`

Money changed from one account's currency into another's:

```json
{ "from_account": "…", "given": "600000", "to_account": "…", "got": "100",
  "occurred_on": "2026-09-25", "description": "cambios on the corner" }
```

Each amount is in its own account's currency. The two amounts are what changed
hands, and the rate of the deal is what they imply — `deal` in the answer, never
stored beside them, because a copy of the rate would disagree with the amounts
the first time either is corrected.

The **day's rate** is the reference instead: the newest rate on or before the
day, from a central bank or one you recorded. What you gave beyond what the
received amount was worth at it is what the bank or the exchange office kept,
and it is recorded as a line of its own under a category the ledger keeps for
the purpose — created as *Exchange fees* the first time, and found by purpose
afterwards, however you rename or move it. A deal better than the reference is a
negative fee, in the same category. The fee is always in the currency you gave.

The answer has the same shape as a converted one-line entry, with
`conversion.fee` absent when the deal matched the reference, and
`conversion.no_fee` saying why when a fee could not be measured:

| `no_fee` | Means |
| --- | --- |
| `no_reference` | no rate for the pair is known on or before the day |
| `stale_reference` | the rate known is older than a long weekend, and would call the currency's own movement a fee |

Both accounts in one currency is a transfer, and is refused.

## Balances

### `GET /api/v1/ledger/balances`

```json
{
  "as_of": "2026-09-17",
  "accounts": [
    { "account_id": "…", "name": "Wallet", "currency": "PYG",
      "amount": "392500.000000000000000000", "converted": "51.025000000000000000" }
  ],
  "total": { "currency": "USD", "amount": "251.025000000000000000" }
}
```

Takes `?as_of=` (today by default) and `?currency=`, which converts each balance
at the rate in force on that day and sums them.

Balances are computed from the entries every time and never stored. `as_of`
includes the day itself — an entry on that date has happened by the end of it.

A balance in a currency with no rate for the day carries **no** `converted`
field, and its currency is named in `total.unconverted`.

A converted balance carries `rate_used` — the rate, the day it was set for, its
source and its age. A currency converted at a rate older than a long weekend is
named in `total.stale`: its money is in the total, at a rate that may no longer
be true.

## Rates

### `POST /api/v1/ledger/rates`

```json
{ "base": "PYG", "quote": "USD", "on_date": "2026-09-17", "rate": "0.00013" }
```

How many of `quote` one `base` buys. `on_date` defaults to today. A rate recorded
one way answers the other way round, inverted, so there is no second row to keep
in agreement.

A rate you record replaces what was recorded for that pair and day. Rates the
central banks publish arrive on their own ([Prices and rates](/austeris/reference/market/))
and never replace anything: a rate that valued a past entry does not change when
a bank revises its history, and one you typed outranks theirs.

### `GET /api/v1/ledger/rates`

Takes `?base=`, `?quote=` and `?limit=` (30 by default). Newest first.

### `GET /api/v1/ledger/rates/at`

The rate in force on a day — what a screen suggests before an amount in another
currency is recorded:

```json
{ "base_currency": "USD", "quote_currency": "PYG", "rate": "5900.28",
  "on_date": "2026-09-24", "source": "bcp", "age_days": 1, "stale": false }
```

Takes `?base=`, `?quote=` and `?on=` (today by default). The answer is the newest
rate not after the day, for the pair as recorded, inverted, or through a third
currency both have a rate with — guaranies in roubles come from the dollar rates
of two central banks, and say so as `"source": "bcp+cbr via USD"`. The freshest
answer wins; on the same day a recorded pair beats a derived one.

`stale` is `true` once the rate is more than four days older than the day asked
about — a weekend with a holiday on either side. It is still answered, because it
is the last thing known; the flag is what keeps it from being read as today's.
`404` when nothing is known on or before the day.

## From a terminal

```console
$ austeris login --email owner@austeris.local
password:
Signed in as owner@austeris.local. The session is kept in …/austeris/session.

$ austeris add 45000 food lunch at the corner
Recorded -45000 PYG on 2026-09-17 - lunch at the corner
```

`austeris add` takes its words unquoted or quoted, and `--on YYYY-MM-DD` for a
day other than today. It calls this API like any other client: the gateway is
the only way in, and the line is parsed by the service, not by the CLI.

A converted line says what the conversion did:

```console
$ austeris add 10 usd subscriptions @5965 streaming from card
Recorded -59650 PYG on 2026-09-25 - streaming
  at 5965 PYG per USD; the day's rate was 5900.28 PYG per USD (bcp, 2026-09-24, a day old); the exchange kept 647 PYG
```

An exchange names the two accounts, each amount in its own account's currency:

```console
$ austeris exchange 600000 cash 100 dollars --note "cambios on the corner"
Exchanged 600000 PYG from Cash for 100 USD into Dollars on 2026-09-25
  at 6000 PYG per USD; the day's rate was 5900.28 PYG per USD (bcp, 2026-09-24, a day old); the exchange kept 9972 PYG
```

`AUSTERIS_URL` points it at an installation other than
`http://127.0.0.1:8084`.
