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

At least two lines, each naming an account **or** a category and never both.
Amounts are signed: negative leaves, positive arrives. `occurred_on` is the day
the money moved — not the instant it was recorded — and defaults to today.

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

The shape is `[+]<amount> [<currency>] <category> [from|to <account>] [note…]`:

| Typed | Means |
| --- | --- |
| `45000 food` | 45 000 spent on `food`, from the only open account |
| `45000 food lunch at the corner` | the same, with everything after the category as the note |
| `45000 food from cash lunch` | …from the account called `cash`; `from` and `to` both name one |
| `+2500000 salary september` | money coming **in**: `+` is the only mark that turns a line around |
| `25000 travel/taxi` | a path, when a bare name means more than one category |
| `1,500.50 food` / `1.500,50 food` | the same amount; thousands separators and either decimal mark |

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

### `GET /api/v1/ledger/entries`

Newest first. Takes `?from=`, `?to=`, `?account=`, `?category=`, `?limit=`
(50 by default, 200 at most) and `?offset=`.

### `DELETE /api/v1/ledger/entries/{id}`

A whole entry with its lines, never one line: half an entry is money from
nowhere.

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

## Rates

### `POST /api/v1/ledger/rates`

```json
{ "base": "PYG", "quote": "USD", "on_date": "2026-09-17", "rate": "0.00013" }
```

How many of `quote` one `base` buys. `on_date` defaults to today. A rate recorded
one way answers the other way round, inverted, so there is no second row to keep
in agreement.

### `GET /api/v1/ledger/rates`

Takes `?base=`, `?quote=` and `?limit=` (30 by default). Newest first.

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

`AUSTERIS_URL` points it at an installation other than
`http://127.0.0.1:8084`.
