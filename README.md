<p align="center"><img src="https://github.com/lacodda/austeris/raw/main/assets/banner.svg" alt="austeris - self-hosted home finance" width="720"></p>

# austeris

Self-hosted home finance for one person's whole picture: accounts and entries in several currencies, what you own and what you owe, loans and deposits, a portfolio of crypto and securities — and on top of it net worth, cash flow and a forecast. One PostgreSQL, a service per module behind a single gateway, a React UI compiled into the binary.

> **Status: skeleton.** This release is the frame, not the product. There is a Cargo workspace, one binary that runs each service as a subcommand, and the plumbing every service will share: configuration, a database pool scoped to the service's own schema, one error shape, and a readiness probe that refuses to report ready against a schema this build does not understand, and migrations that can be rolled back rather than restored from a backup. `docker compose up` brings up PostgreSQL and the gateway, which answers those probes and reserves `/api/v1/{service}/...` — with no services behind it yet. Nothing here books an entry. The first real service is `identity` (v0.3.0), the first prices are `market` (v0.4.0), and the bookkeeping core lands in v0.6.0.
>
> A crypto-portfolio tracker lived in this repository through 2025 and is preserved at the tag [`legacy-2025`](https://github.com/lacodda/austeris/tree/legacy-2025). It is the donor for `market` and `portfolio`, not the code being built on.

## Try it

Requires Docker.

```console
$ git clone https://github.com/lacodda/austeris && cd austeris
$ docker compose up -d
 Container austeris-db-1  Waiting
 Container austeris-db-1  Healthy
 Container austeris-gateway-1  Starting
 Container austeris-gateway-1  Started

$ curl http://127.0.0.1:8084/healthz
{"status":"ok"}

# Readiness, not just liveness - compose waits on this before starting whatever
# depends on the container.
$ curl http://127.0.0.1:8084/readyz
{"status":"ok"}

# A path with no service behind it is unreachable, not proxied nowhere.
$ curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8084/api/v1/ledger/accounts
404
```

Or without Docker, with Rust 1.94 or newer. The gateway owns no schema, so it
starts on a machine that has no database at all:

```console
$ cargo run -- serve gateway
2026-09-01T11:52:40.235679Z  INFO austeris: listening service="gateway" address=0.0.0.0:8080
```

## How it is put together

The shape is fixed by five decisions, each written down in [`docs/adr/`](https://github.com/lacodda/austeris/tree/main/docs/adr):

- **Microservices with guardrails** ([0001](https://github.com/lacodda/austeris/blob/main/docs/adr/0001-microservices-with-guardrails.md)). One workspace, one PostgreSQL, **a schema per service**. A service owns its schema and its migrations and never reads another service's tables — data crosses a boundary only through that service's contract. One gateway is the only public surface.
- **axum, sqlx, PostgreSQL, React** ([0002](https://github.com/lacodda/austeris/blob/main/docs/adr/0002-service-stack.md)). The stack the rest of the line runs on, so techniques transfer.
- **gRPC, no broker** ([0003](https://github.com/lacodda/austeris/blob/main/docs/adr/0003-inter-service-transport.md)). Services call each other synchronously; a broker arrives when a genuinely asynchronous flow does, not in advance.
- **Money is `NUMERIC`** ([0004](https://github.com/lacodda/austeris/blob/main/docs/adr/0004-money-as-numeric-decimal.md)). Decimal in the database, decimal in code, a string on the wire. Never a float — a ledger of floats does not add up.
- **One binary, many services** ([0005](https://github.com/lacodda/austeris/blob/main/docs/adr/0005-one-binary-many-services.md)). `austeris serve <service>` picks which service a process is. Separate processes, separate schemas, separate contracts — one build, one image, one version.

## Configuration

Every variable is prefixed `AUSTERIS_`, and no binary reads a `.env` file: whatever starts the process owns its environment.

| Variable | Default | Meaning |
| --- | --- | --- |
| `AUSTERIS_DATABASE_URL` | — | PostgreSQL connection string. Required by services that own a schema; the gateway runs without it. |
| `AUSTERIS_BIND` | `0.0.0.0:8080` | Address this process listens on. |
| `AUSTERIS_MAX_CONNECTIONS` | `5` | Pooled connections. Several services share one PostgreSQL, so each takes a small slice. |
| `AUSTERIS_ACQUIRE_TIMEOUT_SECS` | `30` | How long a request waits for a free connection. |
| `RUST_LOG` | `austeris=info,tower_http=info,warn` | Log filter. |

Two more are read by `docker-compose.yml` rather than by the binary, for when
the defaults collide with something already running: `AUSTERIS_DB_PORT` (default
`5434`) and `AUSTERIS_PORT` (default `8084`) move the published ports.

Two endpoints exist on every service. `/healthz` says the process is alive and touches nothing, so a database hiccup cannot trigger a restart loop. `/readyz` says it can serve: the database answers and its schema is migrated to the version this build was compiled against.

## Development

```console
$ cargo fmt --check
$ cargo clippy --all-targets -- -D warnings
$ cargo test
```

## License

MIT — see [LICENSE](https://github.com/lacodda/austeris/blob/main/LICENSE).
