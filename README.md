# chat-rs
A Rust playground for exploring type-safe microservices, built as a scalable event-driven chat platform featuring real-time messaging and distributed event streaming.

Currently studying and evaluating the trade-offs of Rust's memory management and type system, particularly the advantages of newtypes and sum types in domain-driven development, and the friction they introduce in projects involving multiple layers of indirection for abstraction and composability, such as services built using hexagonal architecture.

## Quick Start

### Development Environment

The project uses Nix flakes with direnv for automatic environment setup:

```bash
# Install Nix with flakes enabled
# See https://nixos.org/download.html

# Install direnv
# See https://direnv.net/docs/installation.html

# Allow direnv in the project directory
direnv allow
```

The flake template automatically imports all required tools and dependencies. Once direnv is configured, simply entering the project directory activates the complete development environment.

### Running Services

```bash
# Start all services
docker-compose up --build

# Start infrastructure only (for local development)
docker-compose up postgres cassandra kafka

# Run service locally
cd user-service
DATABASE_URL=postgresql://postgres:postgres@localhost:5432/user cargo run
```

## Architecture

Independent services communicate via contracts (proto, REST, events). Each owns its data without shared domain logic.

**Services:**

- **auth** crate provides reusable cryptographic infrastructure for password hashing and JWT validation, shared across services without domain coupling.
- **user-service** owns the user aggregate and authentication domain
- **chat-service** manages channel and message aggregates with Cassandra-backed time-series storage, publishing message events for WebSocket broadcast, and coordinating real-time delivery through persistent connections.

**Hexagonal Architecture**: 

Each service follows hexagonal architecture:
- `src/bin/server/main.rs` — Entry point
- `src/lib/domain/{aggregate}/` — Business logic without serialization or I/O dependencies
  - `models.rs` — Entities, value objects, commands
  - `errors.rs` — Domain error types
  - `events.rs` — Domain events
  - `ports.rs` — Trait definitions
  - `service.rs` — Service implementation
- `src/lib/inbound/` — Drivers (HTTP, gRPC, WebSocket handlers)
- `src/lib/outbound/` — Adapters (database, Kafka, external services)
- `migrations/` — Database migrations

**Storage:**

- PostgreSQL — Users, channels, user replicas (read model)
- Cassandra — Messages (time-series, partitioned by channel_id)
- Kafka — `user-events`, `chat.messages.{0-15}` (16 shards)

**Event Topics:**

*user-events (published by user-service)*
- `UserCreated` → {event_id, user_id, username, email, created_at}
- `UserUpdated` → {event_id, user_id, username, email, updated_at}
- `UserDeleted` → {event_id, user_id, deleted_at}

*chat.messages.{0-15} (published by chat-service)*
- `MessageSent` → {event_id, message_id, channel_id, user_id, content, timestamp}
- `MessageDeleted` → {event_id, message_id, channel_id, deleted_at}

**Eventual Consistency Model:**

chat-service maintains a denormalized `user_replica` table for fast username lookups:
- Populated via Kafka consumer from `user-events` topic
- Upserted on UserCreated/UserUpdated events
- Deleted on UserDeleted events
- Enables message enrichment with username data on read path
- gRPC fallback available for cache misses (user not yet in replica)

For detailed interaction flows, see the [sequence diagrams](./sequence).

**Practices:** 

- Use type system and newtypes
- Use `thiserror` for domain errors
- Use `anyhow` for application errors
- Never `unwrap()` or `expect()` in production code
- Propagate with `?` operator
- User-facing error messages

## API

See [OpenAPI contracts](./openapi) for complete specifications.

## Testing
```bash
./test.sh              # Full integration tests
cargo test --all       # With infrastructure running
```

## License
[Apache 2.0](./LICENSE)
