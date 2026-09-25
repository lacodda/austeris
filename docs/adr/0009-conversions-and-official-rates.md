# 9. Conversions, and official rates pushed to the ledger

Date: 2026-09-25

## Status

Accepted. Extends [0007](0007-entries-balance-by-construction.md), which made
entries balance in every currency, and [0001](0001-microservices-with-guardrails.md),
which keeps the ledger ignorant of its modules.

## Context

ADR 0007 said an exchange is one entry in which each currency sums to zero on
its own. It did not say what the other side of each currency is. The only way to
write one was a dollar line on the guarani wallet and a guarani line on the
dollar account - and since a balance is the sum of an account's lines, the
wallet then held its guaranies minus ten dollars, a number in no currency.

Two things were missing around it. Money spent in a foreign currency from a
guarani card could not be recorded at all: the one-line parser refused it. And
the ledger had no rates it had not been typed: a person in Paraguay with
savings in dollars and family in Russia had to enter three currencies' rates by
hand, every day, for balances to add up.

## Decision

**A line has three kinds of side: an account, a category, or a conversion.** An
account line is in its account's currency, which the database enforces. When
money changes currency, each currency's half lands on the conversion: 60 000
guaranies leave the wallet, 59 003 of them go to the conversion, which gives 10
dollars to the dollar account. The side is a column of its own, not "both
references empty", so a line that lost its reference by mistake fails instead of
reading as a conversion.

**What a deal cost beyond the day's rate is a line of its own.** The day's rate
- the newest one on or before the day - is the reference; the difference between
what was given and what the received amount was worth at it goes to a category
the ledger keeps for the purpose, found by purpose rather than by name. The fee
is in the currency given; a stale or missing reference measures no fee and says
so, rather than calling a week of currency movement a fee. The deal's own rate
is derived from the two amounts and never stored.

**The same mechanism serves an exchange, a foreign purchase and foreign
income.** The category of a 10 USD subscription keeps 10 USD; the card moves by
its own currency; the conversion joins them.

**Official rates come from `market` and are pushed to the ledger.** Banco
Central del Paraguay and the Bank of Russia publish daily tables without keys;
`market` reads them hourly, records them as prices of `fx` instruments, and
hands each table to the ledger over `ledger.v1.RecordRates`. The ledger keeps any
rate it already had for that day - a rate that valued an entry does not change
when a bank revises its history, and one a person typed outranks a bank's. A
pair nobody publishes (guaranies in roubles) is answered through a third
currency both have a rate with.

**Every rate says how old it is.** A daily rate more than four days older than
the day it is used for is stale - one number, in `austeris_common::freshness`,
for the ledger's rates and the market's prices alike. A stale rate is still
used, because it is the last thing known, and is named as stale wherever it is.

## Alternatives

**The category line in the account's currency, with "10 USD" in the note.**
Simpler: no third side, no conversion. But the fact that the thing cost ten
dollars would live in free text, and a report kept in dollars would show a
converted approximation of a number that was exact.

**The ledger asks `market` for a rate when it needs one.** Complete coverage of
any past day, fetched on demand. But the core would then call a module, and
every conversion would depend on `market` answering - the inversion ADR 0001
exists to prevent. Pushing costs a missed day when the ledger is down during a
refresh, which `POST /market/prices/refresh?on=` fills in.

**`reqwest`'s own TLS provider.** The price sources are HTTPS, and until this
release `reqwest` was built without TLS at all - `CoinMarketCap` could never have
answered. Its default provider, `aws-lc-rs`, needs a C toolchain in the arm64
image build; `ring`, already present for sqlx, does not. One function,
`austeris_common::http::client`, installs it, and clippy refuses every other way
of building a client.

## Consequences

- A rollback past this migration is refused while any entry holds a
  conversion: the previous schema cannot express one.
- An account's currency cannot change once it has lines.
- An installation that cannot reach the two banks keeps working; its rates go
  stale, and every answer that uses them says so.
