# Use a multi-arch Rust base image for compatibility with Raspberry Pi (ARM) and x86
ARG RUST_VERSION=1.86
FROM rust:${RUST_VERSION}-slim-bullseye AS builder

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Install sqlx-cli
RUN cargo install sqlx-cli --no-default-features --features postgres

# Set working directory
WORKDIR /usr/src/app

# Copy Cargo files
COPY core_service/Cargo.toml core_service/Cargo.lock ./

# Copy source code and SQLx metadata
COPY core_service/src ./src
COPY core_service/.sqlx ./.sqlx
ARG MIGRATIONS_PATH
COPY ${MIGRATIONS_PATH} ./migrations

# Build the application in release mode
RUN cargo build --release

# Final stage: create a minimal runtime image
FROM debian:bullseye-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libpq5 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary
COPY --from=builder /usr/src/app/target/release/core_service /usr/local/bin/core_service

# Copy sqlx-cli for migrations
COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx

# Copy migrations
COPY --from=builder /usr/src/app/migrations /migrations

# Use environment variable for port
ARG APP_PORT=9000
ENV APP_PORT=$APP_PORT
EXPOSE $APP_PORT

# Run migrations and start the application
CMD sqlx migrate run --database-url "$DATABASE_URL" && core_service