# Build stage
FROM rust:1.96-bookworm AS builder

WORKDIR /app

# Install build dependencies
# - protobuf-compiler: for gRPC/protobuf
# - cmake: for rdkafka-sys
# - libssl-dev, pkg-config: for SSL/crypto
# - libcurl4-openssl-dev: for rdkafka-sys's bundled librdkafka (requires curl headers)
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    libprotobuf-dev \
    cmake \
    pkg-config \
    libssl-dev \
    libcurl4-openssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set sqlx to offline mode (no database needed for compilation)
ENV SQLX_OFFLINE=true

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY contracts/proto/ ./contracts/proto/

# Copy all workspace members (required for workspace build)
# NOTE: Cargo requires ALL workspace members to be present, even when building a single package
COPY crates/auth/ ./crates/auth/
COPY crates/outbox/ ./crates/outbox/
COPY crates/web/ ./crates/web/
COPY services/user-service/ ./services/user-service/
COPY services/chat-service/ ./services/chat-service/

# Build the application
# Note: sqlx compile-time verification disabled for Docker builds
RUN cargo build --release -p user-service

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/user-service /app/user-service

# Copy migrations, config and the dev JWT keypair (dev/test only — real
# deployments mount the keypair from a secret store, not the image).
# --chown to the runtime UID: docker-compose's RUN_MODE=docker config points
# the `server` role straight at this baked-in private key path (no Secret
# mount exists outside Kubernetes), so it must actually be readable at
# runtime — unlike the Kubernetes Deployment, which mounts its own key from
# a Secret and never opens this file at all. The host file itself stays
# 0600 root-owned; --chown only changes ownership inside this image layer.
COPY services/user-service/migrations /app/migrations
COPY services/user-service/config /app/config
COPY --chown=65532:65532 keys/dev /app/keys/dev

# Expose ports
EXPOSE 3001 50051

# Set environment variables
ENV HTTP_PORT=3001
ENV GRPC_PORT=50051

# Numeric, not a named user — bookworm-slim ships no /etc/passwd entry to
# name one, and Kubernetes' runAsUser needs a UID either way. Matches the
# Deployments' own pod securityContext (now defense-in-depth, not the only
# enforcement) and makes the user-migrate Job non-root too, with no
# securityContext of its own needed.
USER 65532:65532

# ENTRYPOINT, not CMD: the user-migrate Job's `args: ["--migrate-only"]`
# needs something to append to. Kubernetes only combines `args` with the
# image's ENTRYPOINT — args with no ENTRYPOINT replaces CMD entirely, and
# the runtime tries to exec "--migrate-only" itself.
ENTRYPOINT ["/app/user-service"]
