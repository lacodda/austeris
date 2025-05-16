# Stage 1: Build dependencies and compile the Rust application
ARG RUST_VERSION=1.86
FROM rust:${RUST_VERSION}-slim-bullseye AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Install sqlx-cli for migrations
RUN cargo install sqlx-cli --no-default-features --features postgres

# Set working directory
WORKDIR /usr/src/app

# Copy Cargo files for dependency caching
COPY core_service/Cargo.toml core_service/Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && \
    echo 'fn main() {}' > src/main.rs && \
    cargo build --release && \
    rm -rf src target/release/deps/*.{rlib,d} target/release/.fingerprint

# Copy source code and SQLx metadata
COPY core_service/src ./src
COPY core_service/.sqlx ./.sqlx

# Copy migrations
ARG MIGRATIONS_PATH
COPY ${MIGRATIONS_PATH} ./migrations

# Build the application in release mode with SQLx offline
RUN cargo build --release --offline

# Stage 2: Create a minimal runtime image
FROM debian:bullseye-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libpq5 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -m -s /bin/bash austeris
USER austeris

# Copy the compiled binary
COPY --from=builder --chown=austeris:austeris /usr/src/app/target/release/core_service /usr/local/bin/core_service

# Copy sqlx-cli for migrations
COPY --from=builder --chown=austeris:austeris /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx

# Copy migrations
COPY --from=builder --chown=austeris:austeris /usr/src/app/migrations /migrations

# Use environment variable for port
ARG APP_PORT=9000
ENV APP_PORT=$APP_PORT
EXPOSE $APP_PORT

# Run migrations and start the application
CMD sqlx migrate run --database-url "$DATABASE_URL" && core_service