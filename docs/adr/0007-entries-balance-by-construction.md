# 7. An entry balances by construction

Date: 2026-09-17

## Status

Accepted.

## Context

The ledger is the core every module posts into, and its shape decides what the
rest of austeris can express. Two shapes were available.

The first is what most budgeting applications store: an entry is an amount, a
category, an account and a sign. It is smaller, it is obvious, and it answers
the common case in one row.

The second is double entry: an entry is a header and its lines, and the lines
sum to zero in every currency they touch.

The first shape stops working at the second thing a person does. A transfer
between two of their own accounts is not income and not an expense, so it needs
a special case — and a `transfer_to_account_id` column that is null on almost
every row. A receipt split across three categories needs a second table, or a
convention where several rows share a group id that nothing enforces. A currency
exchange needs the conversion performed at read time, against a rate that will
have changed by the time anyone reads it.

Each of those is affordable on its own. Together they are four mechanisms where
there could be one, and four places for the same bug.

The 2025 code in this repository is the evidence. It stored a portfolio value in
Redis because recomputing it from transactions was awkward, and then spent its
life being wrong about that value after every write that forgot to invalidate.

## Decision

**An entry is a header and its lines. The lines sum to zero in every currency
they touch. The rule is held by PostgreSQL.**

Four things follow, and they are the decision as much as the rule is:

1. **A line names an account or a category, never both and never neither.** A
   category is the other side of a movement — an income or expense account in
   bookkeeping terms — not an attribute of one. A `CHECK` constraint makes the
   other combinations unexpressible rather than merely wrong.

2. **The balance rule is a deferred constraint trigger, not application code.**
   Deferred because the constraint is about the whole entry and the first line
   of a two-line entry is unbalanced by construction; it is checked at commit.
   In the database because the rule must hold for every writer — a module
   posting through the contract, a person in `psql`, a repair script. A rule
   enforced only by the code that usually writes lasts until something else
   writes. The service checks it too, so a caller is told *which currency is off
   and by how much* instead of a constraint's name; that check is a courtesy,
   and the database's is the guarantee.

3. **No balance is stored.** An account's balance is its opening balance plus
   the sum of its lines, computed per read. A stored balance is a second copy of
   what the lines already say, and on the day the two disagree there is no way
   to tell which is right.

4. **Each currency balances on its own.** An exchange is one entry in which each
   currency sums to zero separately, so the rate is a fact of the entry rather
   than a conversion applied while reading. Without this, ten dollars leaving
   and ten guaranies arriving would "balance".

Rates used for conversion are kept by the ledger rather than read from `market`
on demand, and looked up as of a day: a report of last March must read the same
next year, which it would not if a source revised its history.

## Consequences

Recording a plain expense costs two rows instead of one, and the simplest
possible client must send two lines rather than one amount. That is the price,
and it is paid on every entry.

What it buys: a purchase, a transfer, a split receipt and an exchange are one
mechanism. There is no cache to invalidate and no balance to repair. A module
that posts a wrong entry is refused at the boundary rather than leaving the books
to be reconciled later. And the money in the database cannot be made not to add
up — not by a future module, not by a hurried fix in `psql`.

Reading a balance costs a `SUM` over an index built for it. For one person's
finances, over a decade of entries, that is not a cost worth trading the
guarantee for. If it ever becomes one, the answer is a materialized view the
database maintains — not a column the application remembers to update.
