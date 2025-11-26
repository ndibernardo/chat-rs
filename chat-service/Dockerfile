# Build stage
FROM rust:1.91-bookworm AS builder

WORKDIR /app

# Install build dependencies
# - protobuf-compiler: for gRPC/protobuf
# - cmake: for rdkafka-sys
# - libssl-dev, pkg-config: for SSL/crypto
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    libprotobuf-dev \
    cmake \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set sqlx to offline mode (no database needed for compilation)
ENV SQLX_OFFLINE=true

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY proto/ ./proto/

# Copy all workspace members (required for workspace build)
# NOTE: Cargo requires ALL workspace members to be present, even when building a single package
COPY auth/ ./auth/
COPY user-service/ ./user-service/
COPY chat-service/ ./chat-service/

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

# Copy migrations and config
COPY chat-service/migrations /app/migrations
COPY chat-service/config /app/config

# Expose HTTP port
EXPOSE 3002

# Set environment variables
ENV HTTP_PORT=3002

# Run the application
CMD ["/app/chat-service"]
