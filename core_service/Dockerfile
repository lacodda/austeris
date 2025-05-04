# Use a Rust base image compatible with ARM architecture
FROM rust:1.81-slim-bookworm AS builder

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /usr/src/app

# Copy Cargo files
COPY Cargo.toml Cargo.lock ./

# Copy source code and SQLx metadata
COPY src ./src
COPY .sqlx ./.sqlx
COPY migrations ./migrations

# Build the application in release mode
RUN cargo build --release

# Create a minimal runtime image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libpq5 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary
COPY --from=builder /usr/src/app/target/release/core_service /usr/local/bin/core_service

# Expose the port
EXPOSE 8080

# Run the application
CMD ["core_service"]