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
RUN cargo build --release -p chat-service

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/chat-service /app/chat-service

# Copy migrations, config and the dev JWT keypair (dev/test only — real
# deployments mount the keypair from a secret store, not the image). The
# private half stays 0600 root-owned as copied — no role in this image ever
# opens it (chat-service only ever reads a *public* key, whichever role);
# Config::load just needs the env var's path string non-empty.
COPY services/chat-service/migrations /app/migrations
COPY services/chat-service/config /app/config
COPY keys/dev /app/keys/dev

# Expose HTTP port
EXPOSE 3002

# Set environment variables
ENV HTTP_PORT=3002

# Numeric, not a named user — bookworm-slim ships no /etc/passwd entry to
# name one, and Kubernetes' runAsUser needs a UID either way. Matches the
# Deployments' own pod securityContext (now defense-in-depth, not the only
# enforcement) and makes the chat-migrate Job non-root too, with no
# securityContext of its own needed.
USER 65532:65532

# Run the application
CMD ["/app/chat-service"]
