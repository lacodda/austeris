# Build and run austeris in a container.
#
# One image holds every service. With no argument it runs all of them in one
# process (ADR 0008); `serve <service>` runs one, for an installation with a
# container per service (ADR 0005). The published image is built for amd64 and
# arm64 - the stand is a Raspberry Pi.

FROM rust:1-slim-trixie AS chef
WORKDIR /src
# protoc compiles the service contracts (ADR 0003). The generated Rust is built
# here rather than committed, so a stale checked-in copy cannot disagree with
# the .proto files it came from.
RUN apt-get update \
    && apt-get install --no-install-recommends -y protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked

# The dependency graph, read from the whole workspace rather than listed here:
# a hand-written list of crates is one a new crate silently falls out of, and
# only `docker build` notices - never the gate.
FROM chef AS plan
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
# Dependencies first, from the recipe alone, so editing source does not rebuild
# the whole tree. The proto crate's build script runs even here, so the
# contracts come across with it.
COPY --from=plan /src/recipe.json recipe.json
COPY proto ./proto
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin austeris

FROM debian:trixie-slim
# ca-certificates for TLS to PostgreSQL and to price sources; curl so the
# healthcheck below needs no layer of its own.
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# An unprivileged user: no service needs anything root grants.
RUN useradd --system --create-home --uid 10001 austeris
USER austeris

COPY --from=build /src/target/release/austeris /usr/local/bin/austeris

EXPOSE 8080
# Readiness, not liveness: a process running every service binds this port only
# after each schema is migrated to the version this build expects, and a
# service on its own answers /readyz by checking its schema. Either way a
# container reporting healthy can actually serve.
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/readyz || exit 1

ENTRYPOINT ["austeris"]
CMD ["serve"]
