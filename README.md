<p align="center"><img src="https://github.com/lacodda/austeris/raw/main/assets/banner.svg" alt="austeris - self-hosted home finance" width="720"></p>

# austeris

Self-hosted home finance for one person's whole picture: accounts and entries in several currencies, what you own and what you owe, loans and deposits, a portfolio of crypto and securities — and on top of it net worth, cash flow and a forecast. One PostgreSQL, a service per module behind a single gateway, a React UI compiled into the binary.

> **Status: early.** The books are open. The REST surface describes itself at `/docs`. `docker compose up` brings up PostgreSQL, `identity`, `ledger`, `market` and the gateway; an installation with no accounts creates one and prints its password once. Sessions are rows the server can end, passwords are Argon2id, and an address being guessed at locks out whether or not it exists here. The gateway is the only published port, and **it answers nothing but signing in without a session**: it routes `/api/v1/{prefix}/...` to the service that owns it, validates the session over gRPC and tells that service who is calling. `market` keeps instruments and their prices - decimal to eighteen places, stamped with the instant they were observed, from sources ranked so a second one answers when the first goes quiet. Migrations roll back rather than being restored from a backup.
>
> `ledger` is the bookkeeping core: accounts, a tree of categories, and entries made of **lines that sum to zero in every currency they touch** — a rule PostgreSQL holds, not just the code that usually writes. One rule covers a purchase, a transfer, a receipt split across categories and a currency exchange, and it is why no balance is stored: an account's balance is the sum of its lines. Modules post through `ledger.v1.PostEntry` with an idempotency key, so a salary run that retries does not pay twice. And an expense is one typed line — `austeris add 45000 food lunch` — parsed by the service, so the terminal, the web page and the phone cannot come to disagree about what a sentence means.
>
> A crypto-portfolio tracker lived in this repository through 2025 and is preserved at the tag [`legacy-2025`](https://github.com/lacodda/austeris/tree/legacy-2025). It is the donor for `market` and `portfolio`, not the code being built on.

## Try it

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

```console
$ curl -c jar -X POST http://127.0.0.1:8084/api/v1/auth/login \
    -H 'Content-Type: application/json' \
    -d '{"email":"owner@austeris.local","password":"fcrokhchh87dcpaoquea"}'
{"status":"ok"}

$ curl -b jar -X POST http://127.0.0.1:8084/api/v1/market/instruments \
    -H 'Content-Type: application/json' \
    -d '{"kind":"crypto","symbol":"BTC","name":"Bitcoin","decimals":8}'
{"id":"efafe2e1-506f-4d6f-b13b-ea63b15def93","kind":"crypto","symbol":"BTC","name":"Bitcoin","decimals":8}

# Without a session, nothing behind the gateway answers.
$ curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8084/api/v1/market/instruments
401

# And a path with no service behind it is unreachable, not proxied nowhere.
$ curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8084/api/v1/portfolio/positions
404
```

## Record something

The shortest path from a receipt in your hand to a line in the books:

```console
$ austeris login --email owner@austeris.local
password:
Signed in as owner@austeris.local. The session is kept in .../austeris/session.

$ austeris add 45000 food lunch at the corner
Recorded -45000 PYG on 2026-09-17 - lunch at the corner
```

The line is parsed by the service, not by the CLI, so the terminal, the web page
and the phone cannot drift into reading the same sentence differently. `+` turns
a line around (`austeris add +2500000 salary september`), `from` names an account
when you have more than one, and everything after the category is the note.

What it cannot know, it asks about rather than guesses:

```console
$ austeris add 9000 yachts
Error: you have no category called `yachts`; create it first
```

Underneath, that one line is an entry with two sides. So is a transfer, a receipt
split across three categories, and a currency exchange - the lines of an entry
sum to zero in every currency they touch, and PostgreSQL holds that rule at
commit rather than trusting the code that usually writes:

```console
$ curl -b jar -X POST http://127.0.0.1:8084/api/v1/ledger/entries     -H 'Content-Type: application/json'     -d '{"description":"supermarket","lines":[
          {"account_id":"...","amount":"-50000","currency":"PYG"},
          {"category_id":"...","amount":"30000","currency":"PYG","note":"groceries"},
          {"category_id":"...","amount":"20000","currency":"PYG","note":"bus fare"}]}'

# An entry that does not balance is refused, with the gap named.
$ curl -b jar -X POST http://127.0.0.1:8084/api/v1/ledger/entries     -H 'Content-Type: application/json' -d '{"lines":[...]}'
{"status":400,"error":"Bad Request",
 "message":"the entry does not balance in PYG: the lines sum to -100"}
```

No balance is ever stored - an account's balance is the sum of its lines, and a
total across currencies names the money it could not convert rather than quietly
leaving it out:

```console
$ curl -b jar 'http://127.0.0.1:8084/api/v1/ledger/balances?currency=USD'
{"as_of":"2026-09-17",
 "accounts":[{"name":"Wallet","currency":"PYG","amount":"392500.000000000000000000",
              "converted":"51.025000000000000000"},
             {"name":"Dollars","currency":"USD","amount":"200.000000000000000000",
              "converted":"200.000000000000000000"}],
 "total":{"currency":"USD","amount":"251.025000000000000000"}}
```

The whole REST surface describes itself: `http://127.0.0.1:8084/docs` renders it,
`/openapi.json` is the document, and both answer without a session - a reader has
to see what to call before they have anything to call it with.

Or without Docker, with Rust 1.94 or newer. The gateway owns no schema, so it
starts on a machine that has no database at all:

```console
$ cargo run -- serve gateway
2026-09-01T11:52:40.235679Z  INFO austeris: listening service="gateway" address=0.0.0.0:8080
```

## How it is put together

Microservices sharing one PostgreSQL and one binary behind a single gateway -
a schema per service, gRPC between them, money as `NUMERIC` end to end, and
an entry that balances by construction. The shape is fixed by decisions
written down in [`docs/adr/`](https://github.com/lacodda/austeris/tree/main/docs/adr);
the whole picture: [Architecture](https://lacodda.github.io/austeris/concepts/architecture/).

## Configuration

Every variable is prefixed `AUSTERIS_`, and no binary reads a `.env` file:
whatever starts the process owns its environment. Full reference, including
what each service reads: [Configuration](https://lacodda.github.io/austeris/reference/configuration/).

## Documentation

Full documentation: [lacodda.github.io/austeris](https://lacodda.github.io/austeris/).

## Contributing

Building, tests and repository layout:
[CONTRIBUTING.md](https://github.com/lacodda/austeris/blob/main/CONTRIBUTING.md).

## License

MIT — see [LICENSE](https://github.com/lacodda/austeris/blob/main/LICENSE).
