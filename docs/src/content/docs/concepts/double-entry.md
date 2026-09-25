---
title: Why an entry has sides
description: The one rule the ledger holds, and what it buys.
---

An entry in austeris is a header and its **lines**, and the lines sum to zero in
every currency they touch. That is the whole rule, and everything below follows
from it.

It would have been cheaper to store what most budgeting apps store: an amount, a
category, an account, a sign. That shape works until the first thing that is not
a simple purchase — and then a transfer needs a special case, a split receipt
needs a second table, and a currency exchange needs a conversion done at read
time, on a rate that has since changed.

## What one rule replaces

With sides, those are not four features. They are four shapes of the same thing:

| What happened | The lines |
| --- | --- |
| Bought lunch for 45 000 | `-45000` on the wallet, `+45000` on `food` |
| Moved 60 000 to the bank | `-60000` on the wallet, `+60000` on the bank |
| One receipt, two categories | `-50000` on the wallet, `+30000` on `food`, `+20000` on `transport` |
| Changed 60 000 for 10 dollars | `-60000` on the wallet, `+59003` and `-10` on the conversion, `+10` on the dollar account, `+997` on *Exchange fees* |

A transfer touches no category, which is why moving your own money between your
own accounts never appears in a report of what you spent. A ledger that needs a
category for a transfer reports spending that did not happen.

## Money that changes currency

An account holds one currency, and a line on it is in that currency — the
database refuses anything else, because a balance is the sum of an account's
lines and a dollar line on a guarani wallet would add dollars to guaranies.

So when money changes currency, each currency's half needs somewhere to land.
That is the third kind of side a line can have, besides an account and a
category: a **conversion**. Changing 60 000 guaranies for 10 dollars on a day the
central bank said 5 900.28:

| Line | PYG | USD |
| --- | --- | --- |
| the wallet | −60 000 | |
| *Exchange fees* | +997 | |
| conversion | +59 003 | −10 |
| the dollar account | | +10 |

Each currency still sums to zero on its own. The conversion lines are the two
amounts that were worth the same that day — ten dollars at the day's rate,
rounded to whole guaranies — and what the wallet gave beyond that is what the
exchange office kept, on a line of its own. The rate of the deal, 6 000, is what
the wallet and the dollar account imply, and is never stored beside them.

The same shape covers a 10 USD subscription paid from a guarani card: the card
gives guaranies, the category receives the 10 USD it was charged, and the
conversion joins them. A report kept in dollars sees 10, and the card's balance
moves by what the bank took.

The database holds one more rule here: an entry's conversion lines must take one
currency in and give another out. A conversion that nets to zero in a currency,
or only receives, is money disappearing into a side no report looks at.

## Where the rule lives

In PostgreSQL, as a deferred constraint trigger — not only in the Rust that
usually writes.

It has to be deferred: the constraint is about the whole entry, and the first
line of a two-line entry is unbalanced by construction. It is checked when the
transaction commits, with the entry whole.

It is in the database because the check must hold for **every** writer: a module
posting through the contract, a person in `psql` during a migration, a repair
script written in a hurry. A rule enforced only by the code that usually writes
is a rule that lasts until something else writes. The service checks it too, so
that a caller gets *"the entry does not balance in PYG: the lines sum to -500"*
rather than the name of a constraint — but the service's check is a courtesy and
the database's is the guarantee.

## No balance is stored

An account's balance is the sum of its lines plus its opening balance, computed
on every read. There is no `balance` column, and nothing to recalculate after an
edit.

A stored balance is a second copy of what the lines already say. The day the two
disagree — and they do, after a crash mid-write, a manual fix, a bug in one path
out of five — there is no way to tell which is right. The 2025 code kept a cached
portfolio value in Redis for exactly this reason and spent its life being wrong
about it.

The cost is a `SUM` per read, over an index built for it. For one person's
finances that is not a cost.

## Opening balances are not entries

What was in an account before austeris knew about it lives on the account, not as
an entry with no counterpart. An opening balance is not a movement: booking one
would need a category to book it against, and money that was never earned would
show up in a report of what was.

## Rates are facts of the day

A rate used to value an entry is stored with the day it applied to, and looked up
as "the newest rate not after this day" — Sunday's conversion uses Friday's rate,
because Friday is the last thing anyone observed.

This is why the ledger keeps its own rates rather than asking `market` on every
conversion: a report of last March must read the same next year, and it would not
if a source revised its history. `market` is where a rate is fetched from; the
ledger is where the one that was used is kept.

A rate recorded one way answers the other way round, inverted — `PYG→USD` and
`USD→PYG` are the same fact said twice, and two rows that must agree are two rows
that can disagree. A pair nobody publishes is answered through a third currency
both have a rate with: guaranies in roubles, from two central banks' dollar
rates.

A rate is also as old as the day it was set for, and the ledger says so. Past a
long weekend it is **stale** — still used, because it is the last thing known,
but named as stale wherever it is, so a number from last week is not read as
today's.

## When money cannot be converted

Asking for a total in a currency converts what it can and **names what it could
not**:

```json
{
  "total": { "currency": "USD", "amount": "251.025000000000000000", "unconverted": ["BTC"] }
}
```

A balance with no rate carries no `converted` field at all — not a zero, and not
a copy of its own amount, either of which a reader could add into a total and get
a number that looks like an answer.
