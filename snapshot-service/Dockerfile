# Stage 1: Build austeris-common
ARG RUST_VERSION=1.86
FROM rust:${RUST_VERSION}-slim-bullseye AS common-builder
WORKDIR /usr/src/austeris/austeris-common
COPY austeris-common/Cargo.toml ./
COPY austeris-common/src ./src

# Build austeris-common
RUN cargo build --release

# Stage 2: Build snapshot-service
FROM rust:${RUST_VERSION}-bullseye AS service-builder
WORKDIR /usr/src/austeris/snapshot-service
# Copy austeris-common
COPY --from=common-builder /usr/src/austeris/austeris-common /usr/src/austeris/austeris-common
# Copy snapshot-service
COPY snapshot-service/Cargo.toml ./
COPY snapshot-service/src ./src
# Build snapshot-service
RUN cargo build --release

# Stage 3: Runtime image
FROM debian:bullseye-slim
COPY --from=service-builder /usr/src/austeris/snapshot-service/target/release/snapshot-service /usr/local/bin/snapshot-service

# Use environment variable for port
ARG APP_PORT=8084
ENV APP_PORT=$APP_PORT
EXPOSE $APP_PORT
CMD ["snapshot-service"]