# Stage 1: Build austeris-common
ARG RUST_VERSION=1.86
FROM rust:${RUST_VERSION}-slim-bullseye AS common-builder
WORKDIR /usr/src/austeris/austeris-common
COPY austeris-common/Cargo.toml ./
COPY austeris-common/src ./src

# Build austeris-common
RUN cargo build --release

# Stage 2: Build transaction-service
FROM rust:${RUST_VERSION}-bullseye AS service-builder
WORKDIR /usr/src/austeris/transaction-service
# Copy austeris-common
COPY --from=common-builder /usr/src/austeris/austeris-common /usr/src/austeris/austeris-common
# Copy transaction-service
COPY transaction-service/Cargo.toml ./
COPY transaction-service/src ./src
# Build transaction-service
RUN cargo build --release

# Stage 3: Runtime image
FROM debian:bullseye-slim
COPY --from=service-builder /usr/src/austeris/transaction-service/target/release/transaction-service /usr/local/bin/transaction-service

# Use environment variable for port
ARG APP_PORT=8083
ENV APP_PORT=$APP_PORT
EXPOSE $APP_PORT
CMD ["transaction-service"]