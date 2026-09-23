# 8. One process by default

Date: 2026-09-23

## Status

Accepted. Amends [0005](0005-one-binary-many-services.md), which ran every
service as its own process.

## Context

ADR 0005 put every service in one binary and ran it once per service: a
container for the gateway, one for identity, one for the ledger, one for market.
Each container is a separate process with its own runtime, its own allocator
arenas and its own copy of everything the binary loads.

austeris is installed on a Raspberry Pi that already runs a handful of other
services and three PostgreSQL instances. Four processes where one would do is
memory paid for nothing a household can see, and the number grows with every
module on the plan: portfolio, credit, recurring, forecast.

What the services must keep is not their processes. It is their schemas, their
pools and their contracts (ADR 0001) - and none of those depends on how many
processes there are.

## Decision

**`austeris serve` runs every service in one process; `austeris serve
<service>` runs one.** The published install file starts the first by default
and offers the second as a compose profile, `split`.

In one process, each service still gets its own pool scoped to its own schema,
its own REST listener and its own gRPC listener. The listeners bind loopback
ports chosen by the operating system, and the gateway reaches them over those
ports exactly as it reaches containers over the compose network.

That last point is the choice this record is about. The alternative was calling
each service's router in memory - no sockets, no serialisation, a little faster.
It was rejected because it would make two ways for the gateway to talk to a
service, and the one that is not the default would be the one nobody runs until
it is needed. With loopback, one forwarding path, one session check and one set
of tests serve both shapes; the only thing that differs is a table of addresses
(`Peers`), filled from the environment in one shape and from bound sockets in
the other.

## Consequences

- One container, one runtime, one set of memory arenas on the Pi.
- A panic or a stopped listener in any service ends the whole process, and the
  restart policy brings all of it back. Half an installation answering is worse
  than none: the gateway would forward to a service that is not there.
- Every schema is migrated before any listener opens, so the public port
  answering means the installation is whole.
- Services still cannot read each other's tables: the pools are separate and
  the `search_path` of each is its own schema. Sharing a process shares an
  address space, and nothing in the code reaches across it.
- The demo household (`AUSTERIS_DEMO`) is seeded by the one-process shape only,
  because only it holds every schema. With a container per service it is a
  one-off command, `austeris demo`, and the flag is refused rather than
  silently ignored.
