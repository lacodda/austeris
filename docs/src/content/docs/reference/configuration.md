---
title: Configuration
description: The environment variables austeris reads.
---

Every variable is prefixed `AUSTERIS_`, and no binary reads a `.env` file:
whatever starts the process owns its environment. A stray `.env` in a working
directory must not change how a deployment behaves.

## Every service

| Variable | Default | Meaning |
| --- | --- | --- |
| `AUSTERIS_DATABASE_URL` | — | PostgreSQL connection string. Required by a service that owns a schema; the gateway runs without it. |
| `AUSTERIS_BIND` | `0.0.0.0:8080` | Address this process serves HTTP on - the gateway's, when it runs every service. |
| `AUSTERIS_GRPC_BIND` | `0.0.0.0:9090` | Address a service run on its own serves gRPC to its peers on. Never publish this port. Ignored by `austeris serve` running every service, which binds loopback ports of its own. |
| `AUSTERIS_MAX_CONNECTIONS` | `5` | Pooled connections, per service. Several services share one PostgreSQL, so each takes a small slice - in one process as in several. |
| `AUSTERIS_ACQUIRE_TIMEOUT_SECS` | `30` | How long a request waits for a free connection. |
| `RUST_LOG` | `austeris=info,tower_http=info,warn` | Log filter. |

## Every service in one process

| Variable | Default | Meaning |
| --- | --- | --- |
| `AUSTERIS_DEMO` | `false` | `true` fills an empty installation with a made-up household before starting. Refused when any real person has an account, and refused by a service run on its own - see [Demo](/austeris/reference/demo/). |

## The gateway, run on its own

| Variable | Default | Meaning |
| --- | --- | --- |
| `AUSTERIS_IDENTITY_ADDR` | `http://identity:8080` | Where to forward `/api/v1/auth/...`. |
| `AUSTERIS_IDENTITY_GRPC_ADDR` | `http://identity:9090` | Where to validate sessions. |
| `AUSTERIS_LEDGER_ADDR` | `http://ledger:8080` | Where to forward `/api/v1/ledger/...`. |
| `AUSTERIS_MARKET_ADDR` | `http://market:8080` | Where to forward `/api/v1/market/...`. |

Every routed service has a pair, `AUSTERIS_<SERVICE>_ADDR` and
`AUSTERIS_<SERVICE>_GRPC_ADDR`, so one service can be run outside compose while
the rest stay in it. The defaults are the compose service names, which is what
a container per service uses. A gateway running in the same process as the
services reads none of them: it knows where it put them.

## identity

| Variable | Default | Meaning |
| --- | --- | --- |
| `AUSTERIS_FIRST_USER` | `owner@austeris.local` | The address the first account is created under, on an installation that has none. |
| `AUSTERIS_SECURE_COOKIES` | unset | Set to `true` to mark session cookies `Secure`. Leave it off on plain HTTP, or the browser drops every session cookie and sign-in fails with nothing saying why. |

## market

| Variable | Default | Meaning |
| --- | --- | --- |
| `AUSTERIS_CMC_API_KEY` | unset | CoinMarketCap key. Without it that price source is switched off: the service starts, says so once, and serves whatever prices are already stored. |

## The CLI

| Variable | Default | Meaning |
| --- | --- | --- |
| `AUSTERIS_URL` | `http://127.0.0.1:8084` | Which installation `austeris add` and `austeris login` talk to. Read by the CLI, never by a service. |

## The compose files

Read by compose from `.env`, not by any binary:

| Variable | Default | Meaning |
| --- | --- | --- |
| `COMPOSE_PROFILES` | - | `single` for every service in one container, `split` for a container per service. The install file starts nothing but the database without one of them; `--profile split` on the command line wins over `.env`. |
| `POSTGRES_PASSWORD` | - | The database password, required by the install file. Generate it: `openssl rand -hex 24`. |
| `AUSTERIS_VERSION` | `latest` | Which published image to run. Pin it: an unattended `pull` should not follow `latest`. |
| `AUSTERIS_PORT` | `8084` | The published port - the only one. |
| `AUSTERIS_DB_PORT` | `5434` | Development file only: where PostgreSQL is published for a developer's own tools. |
