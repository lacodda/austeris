# 0006. The contracts describe themselves

Date: 2026-09-01
Status: Accepted

## Context

austeris has two contract surfaces: REST through the gateway, which a browser and any future client speak, and gRPC between services. Both are the kind of thing that rots quietly — a field renamed, a status code changed, a path moved — and the damage is discovered by whoever wrote the client, long after.

The 2025 code had utoipa and a Swagger UI on the monolith. That worked because there was one binary and one surface. With a service per module the same approach produces one spec per service, none of which describes what a client actually faces: a client calls `/api/v1/market/prices`, and the market service's own spec says `/market/prices`.

## Decision

- **Each service annotates its own handlers** (`utoipa::path`) and exports an `ApiDoc`. A service owns the description of its surface for the same reason it owns its migrations.
- **The paths in those annotations are the public ones** — `/api/v1/auth/login`, not the `/auth/login` the service listens on. A document accurate about the compose network and useless outside it is not worth generating.
- **The gateway merges them into one document** at compile time, from the service crates rather than by asking the running services. The binary already contains every service (ADR 0005), so a spec assembled over the network would be the same answer arrived at less reliably — and would go blank whenever a service was down.
- **The gateway serves `/openapi.json` and a viewer at `/docs`, without a session.** A reader has to see what to call before they have anything to call it with, and the shape of this API is public in the repository anyway. The spec describes the surface, never the data behind it.
- **`austeris openapi` prints the document.** The documentation site generates its API reference from that, at build time — a spec checked into the tree is a copy nothing keeps honest.
- **Money is described as a string in the spec**, matching what the JSON actually carries (ADR 0004). A client generated from a spec that says "number" parses a price into a double and loses it before rendering.
- **`buf breaking` guards the gRPC side in CI**, against `main` rather than the previous commit, so a branch cannot break a contract in steps. The answer to needing a breaking change is a new package (`identity.v2`), never an edit to `v1` (ADR 0001).

## Consequences

- Adding an endpoint means annotating it. An unannotated handler is invisible in the spec — which is a real failure mode, so the merge is tested for the paths each service must contribute.
- The documentation site now needs a Rust toolchain to build, because generating the reference means building the binary. That is the cost of the reference never being stale.
- The spec is a published artefact: it must carry nothing personal. utoipa fills `contact` from the manifest's `authors`, which is the owner's address — it is stripped, and a test keeps it stripped.
- `buf` is one more tool in CI, and it only guards `.proto` files. The REST side has no equivalent breaking-change gate; the tests and the spec's own shape checks are what stand in for one until a second client exists.
