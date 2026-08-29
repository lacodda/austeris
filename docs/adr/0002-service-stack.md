# 0002. Service stack: axum, sqlx, PostgreSQL, React

Date: 2026-08-28
Status: Accepted

## Context

The 2025 code is written on actix-web 4 with `log`/`env_logger`, `dotenv` and sqlx 0.8, edition 2021. The other server products of the lacodda line (kasl-server, lyrid) are on axum with `tracing`, sqlx 0.9, edition 2024, and a React SPA compiled into the binary. Techniques proven there — session handling, first-admin bootstrap, backup with a schema version, GHCR images built natively per architecture — transfer only if the stack is the same.

## Decision

- **axum** on tokio for every HTTP surface (gateway and per-service REST), **tonic** for gRPC (ADR 0003).
- **sqlx 0.9** with embedded migrations; **PostgreSQL** with one schema per service. The schema is selected through the connection's `search_path`, not through a `currentSchema` URL parameter (that is JDBC syntax; PostgreSQL ignores it — the 2025 compose relied on it).
- **`tracing`** for logs; configuration from environment variables with the `AUSTERIS_` prefix; no `.env` loading in the binaries (the compose file owns the environment).
- **Edition 2024**, `rust-version` measured (`cargo +VERSION check --all-targets`) and enforced by a CI job that reads it from the manifest.
- **Money is `NUMERIC` in the database and a decimal type in code.** The 2025 schema stored amounts and prices as `FLOAT`; it is not ported as is. (Detailed in ADR 0004 when the first schema lands.)
- **Web UI:** React 19, TypeScript, Vite, Tailwind CSS 4, i18next — the stack shared by kilna and kasl-server — built and embedded into the gateway binary so an install needs no Node.

The actix-web code is ported, not wrapped: repositories, DTOs and SQL move across nearly unchanged; handlers and `main` are rewritten.

## Consequences

- One dialect across the line: an engineer (or an AI session) moving between austeris and kasl-server finds the same shapes.
- Porting costs roughly the 2.3k lines of `core_service` handlers once; the alternative — a permanent second framework in the line — costs on every future change.
- `search_path`-scoped schemas mean sqlx keeps a separate `_sqlx_migrations` table per service schema, which is exactly what per-service ownership of migrations needs.
