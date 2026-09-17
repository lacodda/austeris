<p align="center"><img src="https://github.com/lacodda/austeris/raw/main/assets/banner.svg" alt="austeris - self-hosted home finance" width="720"></p>

> Self-hosted home finance for one person's whole picture: accounts and entries in several currencies, what you own and what you owe, and a portfolio of crypto and securities - on your own machine.

<p align="center">
  <a href="https://github.com/lacodda/austeris/actions"><img src="https://img.shields.io/github/actions/workflow/status/lacodda/austeris/ci.yml?style=flat-square" alt="CI"></a>
  <a href="https://github.com/lacodda/austeris/blob/main/LICENSE"><img src="https://img.shields.io/github/license/lacodda/austeris?style=flat-square" alt="License"></a>
</p>

## Why

A budgeting app knows what you spent. A spreadsheet knows what you own. Neither
knows both, and neither will tell you what you are actually worth this morning
across three currencies, a loan and a handful of coins.

austeris keeps one set of books for all of it - double-entry from the first day,
so every figure it shows is derived rather than typed. No balance is stored: an
account's balance is the sum of its lines, and a total across currencies names
the money it could not convert instead of quietly leaving it out.

## What you get

- **Double entry, enforced by the database.** The lines of an entry sum to zero
  in every currency they touch, and PostgreSQL holds that rule at commit rather
  than trusting the code that usually writes.
- **One line from a receipt to the books.** `austeris add 45000 food lunch at
  the corner` - parsed by the service, so the terminal, the web page and the
  phone cannot read the same sentence differently.
- **Several currencies, honestly.** A transfer, a split receipt and an exchange
  are all the same shape underneath; nothing is converted behind your back.
- **Your whole picture.** Accounts and entries, assets and liabilities, loans
  and deposits, and a portfolio of crypto and securities.
- **A REST surface that describes itself** at `/docs`, so a script of yours is a
  first-class client.
- **On your own machine.** `docker compose up` and it is running; no account
  anywhere else.

## Install

Requires Docker.

```console
$ git clone https://github.com/lacodda/austeris && cd austeris
$ docker compose up -d
 Container austeris-db-1  Healthy
 Container austeris-identity-1  Healthy
 Container austeris-ledger-1  Healthy
 Container austeris-market-1  Healthy
 Container austeris-gateway-1  Started
```

An installation with no accounts creates one and prints its password - once,
into the log of the service that made it:

```console
$ docker compose logs identity

  An account was created, because this installation had none:

      email:    owner@austeris.local
      password: fcrokhchh87dcpaoquea

  This is the only time it is shown. Sign in and change it.
```

## A day in the life

```console
$ austeris login --email owner@austeris.local
password:
Signed in as owner@austeris.local. The session is kept in .../austeris/session.

$ austeris add 45000 food lunch at the corner
Recorded -45000 PYG on 2026-09-17 - lunch at the corner
```

`+` turns a line around (`austeris add +2500000 salary september`), `from` names
an account when you have more than one, and everything after the category is the
note. What it cannot know, it asks about rather than guesses:

```console
$ austeris add 9000 yachts
Error: you have no category called `yachts`; create it first
```

Underneath, that one line is an entry with two sides - and so is a transfer, a
split receipt or an exchange. An entry that does not balance is refused, with
the gap named:

```console
{"status":400,"error":"Bad Request",
 "message":"the entry does not balance in PYG: the lines sum to -100"}
```

Every endpoint, with its shape: **[the ledger reference](https://lacodda.github.io/austeris/reference/ledger/)**.

## Status

Early, and the books are open: entries, accounts, categories and balances work
end to end, with the balance rule held by the database. A crypto-portfolio
tracker lived in this repository through 2025 and is preserved at the tag
[`legacy-2025`](https://github.com/lacodda/austeris/tree/legacy-2025) - it is
the donor for `market`. What landed in each version:
[CHANGELOG](https://github.com/lacodda/austeris/blob/main/CHANGELOG.md).

## Documentation

**[lacodda.github.io/austeris](https://lacodda.github.io/austeris/)** - the API,
configuration, and how it is put together: microservices sharing one PostgreSQL
behind a single gateway, a schema per service, gRPC between them, money as
`NUMERIC` end to end. Architecture decision records are in
[`docs/adr/`](https://github.com/lacodda/austeris/tree/main/docs/adr).

Building it yourself:
[CONTRIBUTING.md](https://github.com/lacodda/austeris/blob/main/CONTRIBUTING.md).

## License

MIT (c) [Kirill Lakhtachev](https://lacodda.com)
