# 0004. Money is NUMERIC in the database and a decimal in code

Date: 2026-09-01
Status: Accepted

## Context

The 2025 schema stored asset amounts and prices as `FLOAT`. Binary floating point cannot represent 0.1, so a column of them does not add up: a hundred entries of ₲45,000 sum to something that is not ₲4,500,000, and the difference grows with the ledger. A bookkeeping product whose balances drift is not a bookkeeping product.

austeris also lives in three currencies at once (PYG, USD, RUB) with wildly different scales — guaraníes have no minor unit and run to seven digits, crypto amounts run to eighteen decimal places. A single fixed scale for every amount does not exist.

## Decision

- **Every monetary or quantity column is `NUMERIC`**, with the scale the domain needs: amounts of money `NUMERIC(20, 4)`, instrument quantities `NUMERIC(38, 18)`, exchange and price rates `NUMERIC(38, 18)`. Never `FLOAT`, `REAL` or `DOUBLE PRECISION`.
- **In code, an amount is a decimal type** (`rust_decimal::Decimal` through sqlx's `rust_decimal` feature), never `f32`/`f64`. This holds across the boundary too: a proto field carrying money is a `string`, not a `double`, because protobuf has no decimal.
- **In JSON, an amount is a string** — `"45000.0000"`, not `45000.0`. A JavaScript client parsing a JSON number gets an IEEE double back and has already lost the value before it renders it.
- **An amount is never separated from its currency.** A column of money is accompanied by a currency column, or lives in a table whose row already fixes one.
- **A conversion is a stored fact, not a recomputation.** When an entry is valued in another currency, the rate used is written down with the entry and dated to the operation; a later report reproduces the number the user saw rather than recomputing it at today's rate.

## Consequences

- Sums, balances and portfolio valuations are exact, and two clients computing the same total agree.
- `NUMERIC` arithmetic is slower than `FLOAT` and the columns are wider. At the scale of one person's finances on a Raspberry Pi this is not measurable.
- Every REST and gRPC contract carries money as a string, so every client needs a decimal library to do arithmetic on it. This is deliberate: a client that cannot do decimal arithmetic should not be doing arithmetic on money.
- The 2025 tables are not ported column-for-column; the legacy import (v0.11.0) converts them and reports the rounding it had to do.
