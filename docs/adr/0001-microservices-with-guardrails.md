# 0001. Microservices with guardrails

Date: 2026-08-28
Status: Accepted

## Context

austeris is a self-hosted home-finance platform: a bookkeeping core (accounts, entries, categories, currencies) with modules for each class of asset or liability — market prices, portfolios, loans and mortgages, deposits, recurring income — and analytics on top. The 2025 code was a single crypto-tracker binary, then a half-started split into services behind an API gateway. The owner's decision for the rebuild is a microservice architecture rather than a monolith: each module is its own service with its own contract and release.

The product is built and run by one person, on a Raspberry Pi. Every extra moving part is paid for at deploy time and at 2 a.m. when something breaks.

## Decision

Microservices, with guardrails that keep the system operable by one person:

- **One Cargo workspace.** Every service is a crate under `crates/`, sharing a lockfile, a toolchain, a lint configuration and a CI pipeline. A cross-service refactor is one commit.
- **One PostgreSQL instance, one schema per service.** A service owns its schema and its migrations and never reads another service's tables. Data crosses a service boundary only through that service's contract.
- **One gateway is the only public surface.** It terminates sessions, routes `/api/v1/{service}/...` to the owning service and serves the web UI. Services listen only on the private compose network.
- **Synchronous calls between services, no message broker** — see ADR 0003. A broker is added by its own release when a genuinely asynchronous flow appears, not in advance.
- **A service is born with its release.** A crate exists only from the version in which it ships functionality. The 2025 tree had four 11-line placeholder services; they are not carried forward.
- **One `docker-compose.yml` for development and one install compose on published images.** Installing the whole platform is one command.

## Consequences

- Adding a module means a crate, a schema, a proto contract, a route prefix in the gateway and a compose entry — a known checklist, repeated per service.
- Reports that span services (net worth, forecasts) are computed by a service that calls the others over gRPC, not by cross-schema SQL. This costs round trips and buys independence of schemas.
- The Pi runs several small processes instead of one; memory per process is the budget to watch. Rust binaries are small, and services that turn out to be idle can be co-scheduled without changing the code.
- The crypto tracker from 2025 becomes two services (`market`, `portfolio`) ported from its code; the tag `legacy-2025` keeps the donor readable.
