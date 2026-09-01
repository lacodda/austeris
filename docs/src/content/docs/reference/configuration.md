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
| `AUSTERIS_BIND` | `0.0.0.0:8080` | Address this process serves HTTP on. |
| `AUSTERIS_GRPC_BIND` | `0.0.0.0:9090` | Address a service serves gRPC to its peers on. Never publish this port. |
| `AUSTERIS_MAX_CONNECTIONS` | `5` | Pooled connections. Several services share one PostgreSQL, so each takes a small slice. |
| `AUSTERIS_ACQUIRE_TIMEOUT_SECS` | `30` | How long a request waits for a free connection. |
| `RUST_LOG` | `austeris=info,tower_http=info,warn` | Log filter. |

## The gateway

| Variable | Default | Meaning |
| --- | --- | --- |
| `AUSTERIS_IDENTITY_ADDR` | `http://identity:8080` | Where to forward `/api/v1/auth/...`. |
| `AUSTERIS_IDENTITY_GRPC_ADDR` | `http://identity:9090` | Where to validate sessions. |

Both exist so one service can be run outside compose while the rest stay in it.
The defaults are the compose service names, which is what a deployment uses.

## identity

| Variable | Default | Meaning |
| --- | --- | --- |
| `AUSTERIS_FIRST_USER` | `owner@austeris.local` | The address the first account is created under, on an installation that has none. |
| `AUSTERIS_SECURE_COOKIES` | unset | Set to `true` to mark session cookies `Secure`. Leave it off on plain HTTP, or the browser drops every session cookie and sign-in fails with nothing saying why. |

## The compose file

Read by `docker-compose.yml`, not by any binary:

| Variable | Default | Meaning |
| --- | --- | --- |
| `AUSTERIS_PORT` | `8084` | The published port - the only one. |
| `AUSTERIS_DB_PORT` | `5434` | Where PostgreSQL is published for a developer's own tools. |
