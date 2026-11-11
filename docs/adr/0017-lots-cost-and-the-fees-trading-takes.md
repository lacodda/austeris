# 17. Lots, what they cost, and the fees trading takes

Date: 2026-11-11

## Status

Accepted. Builds on [0001](0001-microservices-with-guardrails.md),
[0009](0009-conversions-and-official-rates.md) and
[0015](0015-the-portfolio-and-the-money-it-moves.md).

## Context

A position said how many units were held and what they were worth, and not
what they cost: nobody could see what a sale gained, or what a coin held since
spring has gained and not been sold. What a sale gained depends on which
purchases it is counted against, and countries count it differently - the
oldest first, the newest first, or at the average cost of all of them - so the
choice belongs to the person, per account, and has to be changeable without
recording anything again.

Three more things met it. Units came that nobody paid for - a staking reward,
an airdrop, a gift - and there was no trade for them: the 2025 tracker's
"bought at 0" rows were left out of the import, and the sale that followed one
was refused. A fee was booked inside the holding, as part of what a coin cost
(ADR 0015), so the books could not say how much the brokers and exchanges had
taken in a year. And the books, which keep the categories, had no way for a
module to file money under one without first asking the person for its id.

## Decision

**A lot is worked out, never stored.** Every buy and every receive is a lot: so
many units, at a moment, for so much in the account's currency - the price and
the buy's fee, as a cost basis counts it. The lots are what the trades add up to
in the order they happened, worked out whenever they are asked for, as a
position is: a correction to an old trade, or a change of method, is in every
answer from then on, and there is no second copy to part from the first.

**The account picks the lots a sale takes.** `fifo` takes the oldest first,
`lifo` the newest first, `average` a share of every lot by its size - which is
exactly an average cost, while each lot keeps its own moment. First in, first
out until the person says otherwise; the choice is the portfolio's, kept per
account in its own schema, since the ledger knows nothing of lots (ADR 0001).
Changing it counts every gain on the account again, the ones of years past as
well: the method is how the trades are read, not something that happened.

**A transfer carries its lots.** The units it takes - by the leaving account's
method - keep the moment they were acquired and what they cost, and arrive as
lots on the other account. What the network took is units lost on the way: the
units that arrive cost what all those that left did.

**Units received are a trade of their own, `receive`.** Units arrive on one
account, no money moves, and nothing is booked. Its price is what a unit was
worth when it arrived, in the account's currency: what the person says - nothing
at all, for a gift whose worth is not counted - or, when they say nothing, the
market's price of that moment. With neither, the trade is refused and the person
asked: a lot of unknown cost silently counted as free makes every later gain
wrong. The worth is the lot's cost, which is how a reward is counted where it is
income.

**A gain is told in one currency.** A sale's proceeds - its price less its fee -
at the ledger's rate of its day; each lot it took at the rate of the day that lot
was acquired, which is what the money that bought it was worth then. A sale or a
lot in a currency with no rate into the one asked is listed without its numbers,
left out of the sums, and the currency named - as a valuation names what it
could not price. A position's cost, beside what it is worth, is its open lots
counted the same way, and a snapshot keeps it as it was.

**A fee is spending, filed under a category the portfolio keeps.** The entry of
a buy or a sale is now three lines: the account, the holding - the price alone -
and the fee, under the person's category for trading fees. Every currency still
sums to zero, and a report of spending counts the fee and not the coin. What a
lot cost still includes the buy's fee, and a sale's proceeds are net of its own:
that is the cost basis tax is counted on. Adding the fee category to the gains
counts the fees twice.

**A module names its category by a key of its own.** A line may name a
`ModuleCategory` - the module's key, `fees`, with the name and flow it is made
with - in place of a category id. The ledger keeps, per person, which category
is the one for that module and key: made at the root on first use, or a root
category of the person's by that name and flow taken for it rather than a second
made beside it; found by the module and the key afterwards, so the person may
rename or move it. The ledger learns no module's concepts, and the next module
that needs a category of its own - a loan's interest, a salary - needs nothing
new from the contract.

**What was booked before is restated once.** A trade carries which way its money
is booked. When the portfolio starts, it restates in the background the entries
of the trades booked the old way - through the same `RestateEntry` a change of a
trade uses - one trade at a time, locked while it is done, and trying again
later while the ledger or the market is not answering.

## Alternatives

**Lots stored, closed by each sale as it is recorded.** Faster to read. But a
method changed, or an old buy corrected, would have to rewrite every lot and
sale after it - a second copy of the trades kept in step by hand.

**Specific identification - the person picks the lot of each sale.** What some
tax rules allow. It needs a lot named on every sale, and nobody tracking a coin
bought monthly does that; when someone does, it is a fourth method, added later.

**A fee outside the cost basis.** The books and the gains would add up without
overlap. But a gain that leaves the fees out is not the gain any tax return
asks for, and the person would do the arithmetic by hand every spring.

**A purpose of the ledger's own for trading fees.** As the ledger keeps one for
what exchanging money cost. The ledger would grow a purpose for every module's
money, which is the knowledge of modules ADR 0001 keeps out of it.

**A receive's worth always from the market.** No field to fill in. But a gift
counted as free, or a reward the person's tax rules value differently, would
have no way to be said.

## Consequences

- The ledger's contract grows a side of a line, `module_category`; a category
  answers which module, if any, files under it.
- A category is found by the key a typed name is - as a counterparty is - so a
  category of two words can be named in a line.
- The portfolio's API grows `/portfolio/accounts`, `/portfolio/lots` and
  `/portfolio/gains`; a position in a valuation has `cost` and `gain`, and a
  valuation the gain of every position in its total.
- A rollback of the portfolio past this release is refused while a trade of
  kind `receive`, or its history, exists; how accounts count their lots and the
  cost kept on snapshots are forgotten by it, and the books keep the fees where
  they were filed.
- `austeris import legacy` brings a coin the tracker "bought" for nothing as
  units received, worth what the tracker said - nothing - and the sales that
  followed them with it.
