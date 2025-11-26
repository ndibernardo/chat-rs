# chat-rs
A Rust playground for exploring type-safe microservices, built as a scalable event-driven chat platform featuring real-time messaging and distributed event streaming. Currently studying and analyzing the trade-offs of Rust's memory management and type system, particularly the advantages of newtypes and the friction they introduce in projects involving multiple layers of indirection, such as services built using hexagonal architecture.

## Quick Start

### Development Environment

This project uses Nix flakes with direnv for automatic environment setup:

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
### Principles
- Each service is independently deployable
- Services own their data and business logic
- No shared domain logic between services
- Services communicate via contracts (proto, REST, events)

### Components
#### Services
- **auth** crate provides reusable cryptographic infrastructure for password hashing and JWT validation, shared across services without domain coupling.
- **user-service** owns the user aggregate and authentication domain
- **chat-service** manages channel and message aggregates with Cassandra-backed time-series storage, publishing message events for WebSocket broadcast, and coordinating real-time delivery through persistent connections.

#### Project Structure
```
chat-rs/
├── auth/                      # Shared authentication infrastructure
├── user-service/              # User management + JWT
│   ├── src/
│   │   ├── bin/server/        # Entry point
│   │   └── lib/
│   │       ├── domain/        # Business logic
│   │       ├── inbound/       # HTTP and gRPC handlers
│   │       └── outbound/      # Database and Kafka publishers
│   └── migrations/            # Postgres migrations
├── chat-service/              # Chat and channel management
│   ├── src/
│   │   ├── bin/server/        # Entry point
│   │   └── lib/
│   │       ├── domain/        # Business logic
│   │       ├── inbound/       # HTTP and WebSocket handlers
│   │       └── outbound/      # Postgres and Cassandra adapters
│   └── migrations/            # Postgres migrations
├── proto/                     # gRPC contracts
├── scripts/                   # Testing utilities
└── docker-compose.yml
```

#### Hexagonal Architecture
```
service/
├── src/
│   ├── bin/server/main.rs         # Entry point
│   └── lib/
│       ├── domain/                # Business logic
│       │   ├── {aggregate}/
│       │   │   ├── models.rs      # Entities, value objects, commands
│       │   │   ├── errors.rs      # All error types
│       │   │   ├── events.rs      # Domain events
│       │   │   ├── ports.rs       # Trait definitions
│       │   │   └── service.rs     # Service implementation
│       ├── inbound/               # Drivers
│       └── outbound/              # Adapters
├── migrations/                    # Database migrations
└── Cargo.toml                     # [lib] + [[bin]]
```

### System Design

**Data Storage:**
```
PostgreSQL (port 5432)
├── user database → Users table (user-service)
├── chat database → Channels table (chat-service)
└── chat database → User Replica table (chat-service read model)

Cassandra (port 9042)
└── chat keyspace → Messages table (time-series, partitioned by channel_id)

Kafka (16 shards)
├── user-events → User lifecycle events
└── chat.messages.{0-15} → Message events (sharded by channel_id % 16)
```

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

### Code practices and rules
- Use type system and newtypes to make invalid states unrepresentable.
- Use `thiserror` for domain errors
- Use `anyhow` for application errors
- Never `unwrap()` or `expect()` in production code
- Propagate with `?` operator
- User-facing error messages

## API

For complete API specifications with request/response schemas, see the [OpenAPI contracts](./openapi).

### API Reference
*user-service*
- `POST /users` → Register new user
- `POST /users/login` → Authenticate, issue JWT
- `GET /users/{id}` → Get user profile
- `gRPC GetUser()` → Internal user lookup (fallback for replica misses)

*chat-service*
- `POST /channels` → Create channel
- `GET /channels/{id}` → Get channel details
- `GET /channels/{id}/messages` → Query messages (time-range)
- `WebSocket /ws?token={jwt}` → Persistent connection for real-time delivery
  - Client sends: `{"type": "subscribe", "channel_id": "..."}`
  - Server sends: `{"type": "new_message", "id": "...", "user_id": "...", "content": "...", "timestamp": "..."}`

## Testing
### Quick Test
Launch
```bash
./test.sh
```
or with the test infrastructure running `cargo` 
```bash
cargo test --all
```

## Tech Stack
- **Web:** Axum, Tokio
- **Databases:** Postgres (sqlx), Cassandra (scylla)
- **Messaging:** Kafka (rdkafka)
- **RPC:** gRPC (tonic)
- **Auth:** Argon2id, JWT
- **Observability:** tracing, tracing-subscriber

## Future Implementations and ideas

### Fallback Strategy Enhancements
- **Circuit Breaker Pattern** - Prevent cascading failures when gRPC calls fail
- **Retry Policies** - Exponential backoff for transient failures
- **Fallback Chain** - Local cache → Read model → gRPC → Degraded mode
- **Health Checks** - Service availability monitoring for intelligent routing

### Caching Layer
- **Redis Integration** - Distributed cache for frequently accessed data
- **Cache Invalidation** - Event-driven cache updates via Kafka
- **TTL Strategies** - Time-based expiration for user data, channel metadata
- **Cache Warming** - Proactive population of hot data on service startup

### Presence Service
- **User Online Status** - Track active/away/offline states
- **Typing Indicators** - Real-time typing notifications per channel
- **Last Seen Tracking** - Timestamp of last user activity
- **Heartbeat Protocol** - WebSocket ping/pong for connection health

### Infrastructure Improvements
- **Service Mesh** - Istio/Linkerd for observability and traffic management
- **API Gateway** - Centralized routing, authentication, rate limiting

### Type Safety & Contract-First Development
- **OpenAPI Contract Generation** - Explore tools like `utoipa` or `aide` to generate OpenAPI specs from Rust code
- **Contract-to-Code Generation** - Research tooling to generate type-safe API clients from OpenAPI contracts, ensuring compile-time guarantees for API integrations
- **Contract Testing** - Automated validation that implementations match OpenAPI specifications

## License
Apache 2.0
