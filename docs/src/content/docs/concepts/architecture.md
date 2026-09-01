---
title: Architecture
description: How austeris is put together, and why.
---

austeris is a set of small services sharing one PostgreSQL and one binary,
behind a single gateway.

## Services

A service owns a module of the product: `identity` owns people and sessions,
`market` owns instruments and their prices, `ledger` will own accounts and
entries.
Each one:

- owns **its own schema** in the shared database, and never reads another
  service's tables;
- owns **its own migrations**, applied when it starts;
- exposes **REST** through the gateway and **gRPC** to its peers.

Data crosses a service boundary only through that service's contract. A report
spanning services is computed by a service that calls the others, never by SQL
joining two schemas.

## One binary

Every service is the same executable, picked by a subcommand:

```console
$ austeris serve gateway
$ austeris serve identity
```

They are still separate processes with separate ports, schemas and contracts.
What they share is a build, an image and a version - so a deployment cannot end
up running two versions of the platform against one database.

## The gateway

The gateway is the only thing outside the deployment can reach. It:

- routes `/api/v1/{prefix}/...` to the service that owns that prefix - a path
  with no service behind it is unreachable, not proxied nowhere;
- **requires a session** for everything except signing in, validating the cookie
  with `identity` over gRPC and passing the caller's id downstream as
  `x-austeris-user-id`, stripping any such header that arrived from outside;
- caps how fast one client can ask.

Services listen on the private network only. Nothing but the gateway is
published.

## Health and readiness

Every service answers two probes, and they are not the same question:

- `/healthz` - the process is alive. It touches nothing, so a database hiccup
  cannot start a restart loop.
- `/readyz` - the process can serve: the database answers, and its schema is
  migrated to the version this build was compiled against.

Compose waits on `/readyz`, which is what stops a service from starting against
a schema it does not understand.

## Money

Amounts are `NUMERIC` in the database, a decimal type in code, and **strings**
in JSON and protobuf. Never a float: binary floating point cannot represent
0.1, so a column of them does not add up, and a bookkeeping product whose
balances drift is not a bookkeeping product.

A client that wants to do arithmetic on an amount needs a decimal library. That
is deliberate.

## The contracts

Both surfaces describe themselves, and both are guarded.

Each service annotates its own handlers; the gateway merges them into one
OpenAPI document and serves it at `/openapi.json`, with a viewer at `/docs`.
The paths in that document are the public ones - `/api/v1/auth/login`, not the
`/auth/login` the identity service listens on internally. A document accurate
about the compose network and useless outside it is not worth generating.

The merge happens at compile time, from the service crates rather than by asking
the running services: the binary already contains every service, so a spec
assembled over the network would be the same answer arrived at less reliably -
and would go blank whenever a service was down.

The API reference on this site is generated from that document at build time.
A spec checked into the tree is a copy nothing keeps honest.

On the gRPC side, `buf breaking` runs in CI against `main`. A change to an
existing package that would break a client already speaking it is refused; the
answer to needing one is a new package (`identity.v2`), never an edit to `v1`.
