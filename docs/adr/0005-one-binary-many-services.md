# 0005. One binary, many services

Date: 2026-09-01
Status: Accepted

## Context

ADR 0001 chose microservices: a crate per service, a schema per service, a contract per service, one gateway as the only public surface. That decision is about isolating data and contracts. It leaves open a packaging question that arrives with the first Dockerfile — is there an image per service, or one image for the platform?

An image per service is the reflex answer, and it is what the 2025 tree was heading towards: five Dockerfiles, five build jobs, five tags to keep in step. austeris is built and deployed by one person onto one Raspberry Pi. Every image is a build to wait for, a tag to align, and a way for a deploy to end up running two versions of the same platform against one database.

## Decision

**One executable, `austeris`, contains every service.** Which service a process is, is a subcommand:

```
austeris serve gateway
austeris serve ledger
austeris migrate --dry-run
austeris backup | austeris restore
austeris doctor
```

Only `serve gateway` exists as of v0.1.0; the rest is the shape the subcommands land in, service by service, as each is built (`migrate` in v0.2.0, `backup`/`restore` and `doctor` with the deployment release).

- The services stay separate **processes**, each with its own port, its own schema, its own pool and its own contract. Nothing about isolation changes.
- The compose file runs the same image several times with different commands.
- `Service` is one enum in the binary crate — the single place a service is registered, and the gateway's routing table is derived from it.
- Operational commands (`migrate`, `backup`, `restore`, `doctor`) are subcommands of the same binary rather than separate tools, so the version that operates a database is by construction the version that serves it.

## Consequences

- One build, one image, one tag. A deploy cannot mix versions of two services, because there is only one artifact to deploy.
- The image carries code for services a given process does not run. A Rust binary of this shape is single-digit megabytes; the shared base layer saves more than the duplication costs.
- A service cannot be released on its own schedule. This is the real trade-off, and it is acceptable while one person releases the whole platform on one cadence; if that changes, the split is a packaging change, not an architectural one — the crates are already separate.
- `cargo install austeris` gives a working platform, not a fragment.
