# Build and run austeris in a container.
#
# One image holds every service; which one a container is comes from its
# command (ADR 0005). Built on the target machine - the stand is a Raspberry
# Pi, so aarch64 - which also makes this the one place a build for that
# architecture is proven on every deploy rather than only at release time.

FROM rust:1-slim-trixie AS build
WORKDIR /src

# protoc compiles the service contracts (ADR 0003). The generated Rust is built
# here rather than committed, so a stale checked-in copy cannot disagree with
# the .proto files it came from.
RUN apt-get update \
    && apt-get install --no-install-recommends -y protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Dependencies first, so editing source does not re-download and rebuild the
# whole tree. Stub entry points give cargo something to compile them against.
COPY Cargo.toml Cargo.lock ./
COPY crates/austeris/Cargo.toml crates/austeris/
COPY crates/common/Cargo.toml crates/common/
COPY crates/identity/Cargo.toml crates/identity/
COPY crates/proto/Cargo.toml crates/proto/
# The proto crate's build script runs even for a dependency-only build, so its
# inputs come across with the manifests.
COPY crates/proto/build.rs crates/proto/
COPY proto ./proto
RUN mkdir -p crates/austeris/src crates/common/src crates/identity/src crates/proto/src \
    && echo 'fn main() {}' > crates/austeris/src/main.rs \
    && echo '' > crates/common/src/lib.rs \
    && echo '' > crates/identity/src/lib.rs \
    && echo '' > crates/proto/src/lib.rs \
    && cargo build --release \
    && rm -rf crates/austeris/src crates/common/src crates/identity/src crates/proto/src

COPY crates ./crates
# Touch the entry points: cargo skips a rebuild when timestamps look older than
# the artifacts left by the dependency layer.
RUN touch crates/austeris/src/main.rs crates/common/src/lib.rs crates/identity/src/lib.rs crates/proto/src/lib.rs \
    && cargo build --release

FROM debian:trixie-slim
# ca-certificates for TLS to PostgreSQL; curl so the healthcheck below needs no
# layer of its own.
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# An unprivileged user: no service needs anything root grants.
RUN useradd --system --create-home --uid 10001 austeris
USER austeris

COPY --from=build /src/target/release/austeris /usr/local/bin/austeris

EXPOSE 8080
# Readiness, not liveness: /readyz round-trips to the database and checks the
# schema is at the version this build expects, so a container reporting healthy
# can actually serve. Compose waits on this before starting what depends on it.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/readyz || exit 1

ENTRYPOINT ["austeris"]
