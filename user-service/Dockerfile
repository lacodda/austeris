# Stage 1: Build austeris-common
ARG RUST_VERSION=1.86
FROM rust:${RUST_VERSION}-bullseye AS common-builder
WORKDIR /usr/src/austeris/austeris-common
COPY austeris-common/Cargo.toml ./
COPY austeris-common/src ./src

# Build austeris-common
RUN cargo build --release

# Stage 2: Build user-service
FROM rust:${RUST_VERSION}-bullseye AS service-builder
WORKDIR /usr/src/austeris/user-service
# Copy austeris-common
COPY --from=common-builder /usr/src/austeris/austeris-common /usr/src/austeris/austeris-common
# Copy user-service
COPY user-service/Cargo.toml ./
COPY user-service/src ./src
# Build user-service
RUN cargo build --release

# Stage 3: Runtime image
FROM debian:bullseye-slim
COPY --from=service-builder /usr/src/austeris/user-service/target/release/user-service /usr/local/bin/user-service

# Use environment variable for port
ARG APP_PORT=8085
ENV APP_PORT=$APP_PORT
EXPOSE $APP_PORT
CMD ["user-service"]