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

A refusal is `{ "status", "error", "message" }`, the message written for a
person. One a client is meant to act on also carries a `code` and the `params`
the message was written from, so a screen can say it in its own language:

```json
{ "status": 404, "error": "Not Found",
  "message": "you have no category called `yachts`; create it first",
  "code": "line.unknown_category", "params": { "name": "yachts" } }
```

The codes so far are a [second press](#a-second-press), everything a
[typed line](#post-apiv1ledgerentriesquick) can be refused for (`line.*`), a
[file](#files) that is too large, not one that is kept, or there already
(`attachment.*`), and a request larger than the gateway carries
(`request.too_large`). Fall back to `message` for a code you do not know.

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

An entry also says who, where and which labels, all optional:

```json
{
  "counterparty_id": "…",
  "place": { "country": "PY", "city": "Asunción" },
  "tags": ["holiday-2027"],
  "pending": true,
  "lines": ["…"]
}
```

`counterparty_id` is one of your [counterparties](#counterparties); `place` is
found or created by what it is ([places](#places)); each tag is found by name in
any case, or created ([tags](#tags)). `pending: true` records a payment the bank
has not posted yet - see [held payments](#held-payments).

Every entry comes back with all of it: `status` (`pending` or `cleared`),
`cleared_on` (the day the bank posted it; absent while held), `counterparty`
(`id` and `name`), `place`, and `tags` in alphabetical order.

### A second press

An entry you record is refused with `409` when the same one was recorded in the
last minute: the same day, description and counterparty, and the same lines -
every account and category moved by the same amounts. A button pressed twice, a
request retried, a press again because nothing seemed to happen all send exactly
that; a second purchase almost never does.

```json
{ "status": 409, "error": "Conflict",
  "message": "the same entry was recorded 4 seconds ago; if this is a second one, send it again with `allow_duplicate`",
  "code": "entry.possible_duplicate", "params": { "entry_id": "…", "seconds": 4 } }
```

Send it again with `"allow_duplicate": true` when it is a second one. Every way
of recording takes it - an entry, a [typed line](#post-apiv1ledgerentriesquick),
an [exchange](#exchanges), a [template](#templates), an entry
[recorded again](#post-apiv1ledgerentriesidrepeat). Two presses that arrive at
once are one entry and one refusal: a person's entries take turns being checked.
A module's entries carry idempotency keys, and a retry of one is answered from
its key instead.

### `POST /api/v1/ledger/entries/quick`

One typed line, which is the fastest way there is to record an expense:

```json
{ "text": "45000 food lunch at the corner" }
```

The shape is `[~][+]<amount> [<currency>] [<category>] [note…]`, with any of
`from|to <account>`, `@<rate>`, `@<counterparty>` and `#<tag>` anywhere after the
amount. The category is the first word that is none of those, and every plain
word after it is the note:

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
| `45000 food @casarica lunch` | paid to the counterparty *Casa Rica* |
| `45000 @casarica` | …filed under its usual category |
| `45000 food #holiday-2027` | labelled with a tag |
| `~300000 hotel from card` | a payment the bank is still holding |

After `@`, a word starting with a digit is a rate and anything else is a
counterparty - so `@59oo` is refused as a mistyped rate rather than taken for a
shop called *59oo*. A counterparty is matched without case, spaces or
punctuation: `@casarica`, `@Casa-Rica` and `@casa_rica` are all *Casa Rica*. A
rate or a counterparty said twice is refused.

A counterparty named for the first time is **created**, with the line's category
as its usual one, and the answer says so in `new_counterparty` - so the next
line can be `45000 @casarica`, and a mistyped name is seen the first time. A
line that names only a counterparty needs one with a usual category; one never
used before needs a category beside it.

A tag is `#` and a word starting with a letter - `order #1234` stays a note about
an order. One used for the first time is created and listed in `new_tags`.

A leading `~` holds the payment: it is out of what is available, and not in the
balance until it [posts](#held-payments). With `+`, either order: `~+5000` and
`+~5000` are the same.

`-45000` is **not** accepted as the opposite of `+`. An expense is what a bare
line already means, and a second spelling of the common case is how a `-` typed
out of habit comes to record income.

The place goes beside the text rather than in it, like the day - it is where you
are, not something you say about each purchase:

```json
{ "text": "45000 food lunch", "place": { "country": "BR", "city": "São Paulo" } }
```

So does the account, for a screen where it is already chosen: `account_id` is
the account a line that names none is recorded against. A line that says
`from cash` is still taken at its word.

```json
{ "text": "45000 food lunch", "occurred_on": "2026-10-11", "account_id": "…" }
```

What the parser cannot know is resolved against your own books, and refused
rather than guessed:

- a category you do not have → `404`, naming it;
- a name meaning two categories → `409`, listing them;
- two open accounts and no `from` (and no `account_id`) → `400`, listing them;
- `45000 salary`, an expense filed under an income category → `400`.

Each refusal carries a `line.*` code. The same parser serves `austeris add` and
the [web interface](/austeris/reference/web/): a sentence means the same thing
wherever it is typed.

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

### `POST /api/v1/ledger/entries/quick/preview`

What a line would record, worked out exactly as recording it would be and
written nowhere - not the entry, not a counterparty or a tag the line names for
the first time, not the category a fee would go under. A screen asks it while the
line is typed, so what the line means is seen before anything is pressed. Takes
the same body as recording it:

```json
{
  "occurred_on": "2026-10-11",
  "flow": "expense",
  "account": { "id": "…", "name": "Cash", "currency": "PYG", "…": "…" },
  "category": { "id": "…", "name": "food", "flow": "expense" },
  "amount": { "amount": "45000", "currency": "PYG" },
  "moves": "-45000",
  "description": "lunch",
  "counterparty": { "name": "Casa-Rica" },
  "tags": ["trip"],
  "new_tags": ["trip"],
  "pending": false
}
```

`moves` is what the account would move by, signed, in its own currency;
`amount` is what the line said. A counterparty without an `id` is a name
recording would create. A line that would be refused is refused here, with the
same status and code.

### `GET /api/v1/ledger/entries`

Newest first. Takes `?from=`, `?to=`, `?account=`, `?category=`, `?tag=` (a
name, in any case), `?counterparty=`, `?place=`, `?status=` (`pending` or
`cleared`), `?limit=` (50 by default, 200 at most) and `?offset=`.

### `PATCH /api/v1/ledger/entries/{id}`

Changes what an entry says about itself - never its lines:

```json
{ "description": "dinner", "counterparty_id": null, "tags": ["holiday-2027"] }
```

Takes `occurred_on`, `description`, `counterparty_id`, `place` and `tags`. A
field left out stays as it is; `null` clears the counterparty or the place, and
`tags` replaces every tag the entry had (`[]` takes them all off). An entry
posted the day it happened moves its posting day with it.

### `DELETE /api/v1/ledger/entries/{id}`

A whole entry with its lines, never one line: half an entry is money from
nowhere. It goes to [the trash](#the-trash) with its files: gone from every
balance, total and list at once, and back as it was if it is restored.

### `POST /api/v1/ledger/entries/{id}/repeat`

Records an entry again - yesterday's bus fare, today:

```json
{ "occurred_on": "2026-10-12" }
```

The copy says what the entry says - its lines, description, counterparty, place
and tags - on the day given, today when it is left out. It is held when the
entry went through a hold: a payment that was held once is the kind that is.

A payment charged in another currency keeps what it cost there and is converted
again at the day's rate: the rate is a fact of its day, and the old one is a
number nobody was charged. An exchange between two accounts is refused - what
changes hands is the deal, and the next one is not the same. The answer is the
new entry, and `conversion` when it was converted again.

## Templates

Buttons that record the same payment again: the coffee, the bus, the rent.

A template is made **from an entry** rather than filled in: the entry already
says which account, category, counterparty, place and tags. It is kept by what
it points at, so a category or a counterparty renamed later is still the one it
means. It is one payment - one account, one category, and the conversion between
them when it was charged in another currency; a split receipt, a transfer or an
exchange is [recorded again](#post-apiv1ledgerentriesidrepeat) instead.

### `POST /api/v1/ledger/templates`

```json
{ "entry_id": "…", "name": "Coffee", "ask_amount": false }
```

`ask_amount: true` makes a template for a place where every receipt is different:
it keeps everything but the amount, and asks for one each time. It is held when
the entry was. The template comes back as the last button:

```json
{ "id": "…", "name": "Coffee", "account_id": "…", "category_id": "…",
  "flow": "expense", "amount": "15000", "currency": "PYG", "description": "coffee",
  "counterparty": { "id": "…", "name": "Cafe" }, "place": null, "tags": ["work"],
  "pending": false }
```

`amount` is absent on one that asks. Each name is one button: a second
`Coffee`, in any case, is `409` with the code `template.name_taken`.

### `POST /api/v1/ledger/templates/{id}/use`

```json
{ "occurred_on": "2026-10-12", "amount": "18000" }
```

Records the template's payment on the day given, today when it is left out.
`amount`, in the template's currency, is needed when it asks for one and
replaces its own for this once otherwise. An amount in another currency than the
account's is converted at the day's rate. The answer is the entry, as for a
typed line. A second press within a minute is [refused](#a-second-press) like
any other.

### `GET /api/v1/ledger/templates`

Every template, in the order the buttons sit.

### `PATCH /api/v1/ledger/templates/{id}`

```json
{ "name": "Superseis", "amount": null }
```

Renames it, or gives it an amount - `null` makes it ask each time. What it
records is the entry it was made from; to change that, make another.

### `PUT /api/v1/ledger/templates/order`

```json
{ "ids": ["…", "…", "…"] }
```

Every template of yours, each once, first button first; an order that leaves one
out is refused.

### `DELETE /api/v1/ledger/templates/{id}`

The entries it recorded stay: they are your money, and the button was only a
way of writing them.

## Held payments

A card payment leaves the moment it is made and is posted days later, often for
a different amount - a hotel's pre-authorisation, a fuel pump's, a charge in
another currency converted at the bank's rate on the day it posts. Until then it
is **held**: recorded with `"pending": true` or a leading `~`, it is out of what
is [available](#balances) and not yet in the balance.

### `POST /api/v1/ledger/entries/{id}/clear`

```json
{ "on": "2026-09-12", "amount": "180000" }
```

Posts a held entry on `on` (today by default; not before the day it happened).
With `amount` - positive, in the account's currency - it posts for what the bank
actually took instead of what was held:

- a plain payment's category moves with it;
- a payment in another currency keeps what it cost in that currency - the
  10 USD stay 10 USD - and the conversion between the two is worked out again,
  with the [fee](#exchanges) measured against the rate on the day it posted;
- a payment from two accounts, or one split across categories, does not say
  which part changed, and is refused: record it again instead.

The answer is the entry, with `conversion` when it was worked out again. `409`
when it was posted already.

A balance as of a past day reads the same before and after a payment posts: the
day it posted is kept, so a payment posted on the 12th was held on the 11th.

## History

### `GET /api/v1/ledger/entries/{id}/history`

Everything that happened to an entry, oldest first - for an entry in the books
and for one in the trash alike:

```json
[
  { "id": 41, "at": "2026-10-12T14:03:11Z", "action": "created",
    "who": { "kind": "person", "person_id": "…" }, "changed": [],
    "before": null, "after": { "description": "lunch", … } },
  { "id": 57, "at": "2026-10-14T09:20:45Z", "action": "changed",
    "who": { "kind": "person", "person_id": "…" },
    "changed": ["status", "lines"],
    "before": { "status": "pending", "lines": [ … ] },
    "after": { "status": "cleared", "cleared_on": "2026-10-14", "lines": [ … ] } }
]
```

`action` is `created`, `changed`, `deleted` or `restored`; `before` and `after`
are the entry as [an entry](#get-apiv1ledgerentries) is shown, absent where it
did not exist yet or no longer did. `changed` names the parts a change touched,
in the order an entry is read: `occurred_on`, `description`, `status`,
`counterparty`, `place`, `tags`, `lines`, `attachments`. A part is compared by
what it is, not by how it reads: a tag renamed since is the same tag.

`who` is a `person`, a `module` (with the `module` name it signed with) or
`outside` - a write to the database that did not come through austeris, psql or
a repair script. The history is kept by the database itself, so such a write is
recorded like any other, and one request is one revision however many rows it
touched. A save that changed nothing is not one.

The history begins with 0.12: an entry recorded before then begins with its
being recorded, by the person or the module that did.

## Files

A photo of the receipt, the bank's PDF, the receipt's XML - kept with the entry
they prove. An entry lists what it has in `attachments`, without the bytes:

```json
"attachments": [
  { "id": "…", "name": "receipt.jpg", "media_type": "image/jpeg", "size": 2482113,
    "created_at": "2026-10-12T14:05:02Z" }
]
```

### `POST /api/v1/ledger/entries/{id}/attachments?name=receipt.jpg`

The body is the file as it is. What it is is read from its bytes - a JPEG, PNG,
WebP or HEIC picture, a PDF, an XML document - whatever it is called or sent
as, and anything else is refused with `415` and `attachment.unsupported`. Ten
megabytes at most (`413`, `attachment.too_large`, with `limit_mb`); the same
file on the same entry twice is refused with `409` and
`attachment.already_attached`, naming the copy already there.

```console
$ curl --cookie "$SESSION" --data-binary @receipt.jpg \
    "https://austeris.example/api/v1/ledger/entries/$ENTRY/attachments?name=receipt.jpg"
```

### `GET /api/v1/ledger/attachments/{id}/content`

The file, under its own name. A picture or a PDF opens in place; an XML
document is only ever saved - one in the XHTML namespace is a page, and a page
from this address would run with the session that fetched it. Readable while
the file is on an entry or waiting in the trash.

### `DELETE /api/v1/ledger/attachments/{id}`

Takes a file off its entry, to [the trash](#the-trash). A file of an entry that
is in the trash is not taken off on its own: it goes back with the entry, or is
erased with it.

## The trash

Deleted entries, and files taken off entries, wait thirty days; then they are
erased for good - an entry with its history and its files. Erasing runs every
hour in the ledger's own process, so a trash nobody opens does not keep a
season of receipts.

### `GET /api/v1/ledger/trash`

```json
{ "keep_days": 30,
  "entries": [
    { "entry": { "id": "…", "description": "lunch", "attachments": [ … ], … },
      "deleted_at": "2026-10-14T18:02:10Z", "who": { "kind": "person", "person_id": "…" },
      "erased_after": "2026-11-13T18:02:10Z" } ],
  "attachments": [
    { "attachment": { "id": "…", "name": "scan.pdf", … }, "entry_id": "…",
      "entry_title": "Pharmacy", "entry_occurred_on": "2026-10-02",
      "removed_at": "2026-10-14T18:05:44Z", "erased_after": "2026-11-13T18:05:44Z" } ] }
```

The most recently deleted first. `attachments` holds the files taken off
entries still in the books; a deleted entry's files are on the entry.

### `POST /api/v1/ledger/trash/entries/{id}/restore`

Writes the entry back as it was deleted: the same id, the same lines under the
same ids, its first day of being recorded, its files. Its counterparty, place
and tags are the ones it had - found again by name, the way typing them would,
when one was deleted while the entry waited. Refused with `409` when it moved
money on an account or under a category that no longer exists, or when the
module that posted it has posted it again since.

### `DELETE /api/v1/ledger/trash/entries/{id}`

Erases a deleted entry now, with its history and its files.

### `POST /api/v1/ledger/trash/attachments/{id}/restore`

Puts a file back on its entry. Refused with `409` when the entry is in the trash
too - restore the entry, and its files come with it - or when the same file has
been attached to it again since (`attachment.already_attached`).

### `DELETE /api/v1/ledger/trash/attachments/{id}`

Erases a file taken off an entry, now.

### `DELETE /api/v1/ledger/trash`

Erases everything in the trash.

## Counterparties

Who money goes to or comes from: a shop, a service, a person, an employer.

### `GET /api/v1/ledger/counterparties`

```json
[{ "id": "…", "name": "Casa Rica", "key": "casarica", "kind": "shop",
   "default_category_id": "…", "tax_id": "80012345-6" }]
```

`key` is how it is typed after `@`: the name in lower case without spaces or
punctuation. Derived, never set - and unique, so *Casa Rica* and *casa-rica*
cannot be two counterparties splitting what was spent there.

`tax_id` is the shop's tax number as its receipts print it - a Paraguayan RUC
with its check digit. A receipt finds its shop by it before its name, whatever
name the receipt is printed under, and a shop typed by hand gains it from its
first receipt ([Imports](/austeris/reference/imports/)). Unique too: two
counterparties cannot carry one number.

### `POST /api/v1/ledger/counterparties`

```json
{ "name": "Casa Rica", "kind": "shop", "default_category_id": "…" }
```

`kind` is `shop`, `service`, `person` or `organisation`, and is left unsaid when
omitted - one created from a typed line is not known to be a shop.
`default_category_id` is what money spent there usually is for. `409` when you
have one typed the same way.

### `PATCH /api/v1/ledger/counterparties/{id}`

Takes `name`, `kind`, `default_category_id` and `tax_id`; `null` clears the
last three.
Every entry naming it follows: they name the counterparty, not a copy of its
name.

### `DELETE /api/v1/ledger/counterparties/{id}`

Only one that no entry names; `409` otherwise.

## Places

A country, as its ISO 3166-1 code, and a city in it when one is said. A code that
only looks like one is refused - the United Kingdom is `GB`, not `UK`. A city is
one place however its accents are typed: `ASUNCION` on a receipt is the
Asunción already in your books.

### `GET /api/v1/ledger/places`

Every place you have recorded something at.

### `PATCH /api/v1/ledger/places/{id}`

```json
{ "country": "PY", "city": "Asunción" }
```

Renames it on every entry that happened there at once. A place is found by what
it is, in any case, so *asuncion* typed twice is one place; `409` when you
already have the one it is renamed to.

## Tags

Your own labels across categories: `#holiday-2027` on the flights, the hotel and
the dinners, whatever each was filed under. A tag is one word starting with a
letter, then letters, digits, `-` and `_`.

### `GET /api/v1/ledger/tags`

Every tag, by name.

### `PATCH /api/v1/ledger/tags/{id}`

```json
{ "name": "holiday-2028" }
```

### `DELETE /api/v1/ledger/tags/{id}`

Takes the tag off every entry that carried it. The entries stay, and each one's
history says the tag came off.

## Totals

### `GET /api/v1/ledger/totals`

What money was spent on and earned from, gathered:

```json
{
  "by": "counterparty",
  "groups": [
    { "id": "…", "name": "Casa Rica",
      "amounts": [{ "currency": "PYG", "spent": "1250000", "earned": "0" }] },
    { "id": null, "name": null,
      "amounts": [{ "currency": "PYG", "spent": "310000", "earned": "2500000" }] }
  ]
}
```

`?by=` is `category`, `tag`, `counterparty`, `place` or `country`. It takes
`?from=`, `?to=`, and narrows to `?tag=`, `?counterparty=` or `?account=`:

| Asked | Answers |
| --- | --- |
| `?by=counterparty` | who got how much, and who paid how much |
| `?by=tag` | what each tag has gathered so far |
| `?by=category&tag=holiday-2027` | what the holiday went on |
| `?by=country&from=2026-01-01` | what was spent in each country this year |

Read from the lines on categories, which is what money is for: a transfer
between your own accounts moves nothing here, and an exchange's fee is spending
like any other. `spent` is the expense lines less refunds; `earned` the income
lines. Each group is reported in every currency it moved in and never added
across them. Held payments count on the day they were made.

The entries that name no counterparty, tag or place are reported too, last, as
a group with no `id` and no `name`, so the groups account for every line. An
entry with two tags counts under both.

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
      "amount": "392500.000000000000000000", "available": "362500.000000000000000000",
      "converted": "51.025000000000000000", "converted_available": "47.125000000000000000" }
  ],
  "total": { "currency": "USD", "amount": "251.025000000000000000",
             "available": "247.125000000000000000" }
}
```

Takes `?as_of=` (today by default) and `?currency=`, which converts each balance
at the rate in force on that day and sums them.

Balances are computed from the entries every time and never stored. `as_of`
includes the day itself — an entry on that date has happened by the end of it.

`amount` is what the bank had posted by the end of the day - the number the
account's statement shows. `available` is that less the [payments still
held](#held-payments) on it: money that has left and not been posted. Money held
on its way **in** is not added until it arrives, which is how a bank counts it.

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

The same line twice within a minute is taken for one typed twice:

```console
$ austeris add 15000 food coffee
Recorded -15000 PYG on 2026-10-12 - coffee
$ austeris add 15000 food coffee
Error: the same entry was recorded 6 seconds ago - typed twice? If this is a second one, run it again with --again
$ austeris add 15000 food coffee --again
Recorded -15000 PYG on 2026-10-12 - coffee
```

`austeris exchange` takes `--again` the same way.

A counterparty, tags and a hold are printed back the way the line said them,
and anything named for the first time is pointed out:

```console
$ austeris add "~45000 @Casa-Rica food #holiday-2027 lunch" --country PY --city Asunción
Recorded -45000 PYG on 2026-09-17 @Casa-Rica #holiday-2027 (held) - lunch
  new counterparty Casa-Rica, typed as @casarica
  new tag #holiday-2027
```

A shell reads `#` as the start of a comment, and PowerShell reads a leading `@`
as its own: quote a line that has either.

`austeris pending` lists what is held, with the start of each id; `austeris
clear` posts one, for what the bank took when that was another amount:

```console
$ austeris pending
3f2a91c0  -45000 PYG on 2026-09-17 @Casa-Rica #holiday-2027 - lunch

$ austeris clear 3f2a 47500 --on 2026-09-19
Posted -47500 PYG on 2026-09-19, made on 2026-09-17 @Casa-Rica #holiday-2027 - lunch
```

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
