---
title: Demo
description: A made-up household, for a first look and for screenshots.
---

The demo fills an empty installation with a household that does not exist, so
there is something to look at before anything real is typed - and so
screenshots never carry a real number.

## Two ways to ask for it

On an installation running every service in one process (the default), set
the flag and start:

```ini
AUSTERIS_DEMO=true
```

With a container per service no one process holds every schema, so the flag is
refused there, and the demo is a one-off command on an empty database instead:

```console
$ docker compose --profile split run --rm identity demo
$ docker compose --profile split up -d
```

Either way it prints how to sign in:

```text
  A demo household was created. Everything in it is made up.

      email:    demo@austeris.local
      password: austeris-demo
```

## What is in it

- **Accounts:** `Everyday` (bank, EUR), `Cash` (EUR), `Savings` (deposit, EUR)
  and `Travel card` (card, USD).
- **Categories:** `Salary` and `Side work` coming in; `Home` (`Rent`,
  `Utilities`), `Food` (`Groceries`, `Eating out`), `Transport`, `Health`,
  `Fun` and `Trips` going out.
- **Four months of entries** up to today: a salary on the first, rent on the
  third, groceries every few days, weekly trips to the cash machine and a
  monthly transfer into savings - transfers are entries like any other.
- **A dollar rate for every day**, so balances in two currencies add up.
- **Two instruments nobody trades**, `DMC` and `DWLD`, with a price a day. Their
  names are invented on purpose: a demo price can never be mistaken for a real
  one.

Everything it writes is marked with the source `demo`.

## What it will not do

- **Share an installation with real people.** If anyone but the demo household
  has an account, it refuses and says so. The prices it writes are shared by
  everyone on an installation, and made-up prices must never reach real books.
  Start from an empty database to see it.
- **Duplicate anything.** Each day's entries carry a key made from the day, and
  accounts and categories are found by name before they are created. Running it
  again - or restarting with the flag still set - adds the days since the last
  run and nothing else.
