# 0003. Inter-service transport: gRPC, no broker

Date: 2026-08-28
Status: Accepted

## Context

Services need each other: the gateway validates a session with `identity`; `portfolio` prices positions through `market`; `credit` and `recurring` post entries into `ledger`; `forecast` reads from all of them. The 2025 split had started on gRPC with tonic (`asset-service` exposed `GetPrice`). The alternatives were internal HTTP/JSON, or gRPC plus a message broker (NATS) from the first release.

## Decision

- **gRPC over tonic for every call between services.** Contracts live in `proto/{service}/v1/*.proto` at the repository root, compiled by each crate's `build.rs`; the package version is part of the path, so a breaking change is a new package, not an edited one.
- **No message broker.** Every cross-service interaction in the roadmap up to 1.0 is request/response: price lookup, entry posting, session validation, report assembly. A broker is added by its own release when a flow appears that is genuinely asynchronous (a price feed fanning out to several consumers, notifications) — and the decision is recorded then, with the flow that justified it.
- **The gateway speaks REST/JSON to the outside and gRPC to the inside.** External clients never reach a service's gRPC port; services never expose REST directly except through the gateway's route table.
- **Idempotency belongs to the callee.** A service that creates something on behalf of another (ledger entries posted by portfolio or credit) accepts a caller-supplied idempotency key, so a retried call after a timeout does not double-post.

## Consequences

- Typed contracts at every boundary; a field renamed in a proto fails to compile on both sides instead of returning `null` at runtime.
- One fewer container on the Pi, and no "eventually" in the data model until a release explicitly introduces it.
- Fan-out reports (net worth across ledger, portfolio and credit) are assembled by sequential or parallel gRPC calls; if that ever becomes the bottleneck, a materialized read model is the next step, not a broker.
- Debugging inside the network needs `grpcurl` rather than `curl`; the gateway's REST surface remains the everyday tool.
