# chat
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
cd services/user-service
DATABASE_URL=postgresql://postgres:postgres@localhost:5432/user cargo run
```

## Project Structure

```
crates/          # Shared library crates 
services/        # user-service, chat-service — each hexagonal, each owns its data
contracts/proto/ # Shared gRPC contract
deploy/          # Kubernetes: operators, Helm charts, kind config, Argo CD
scripts/         # Local Docker Compose tests + Kubernetes dev/staging lifecycle
keys/            # Committed dev-grade JWT keypairs (dev and staging only)
```

## Architecture

Independent services communicate via contracts. Each owns its data without shared domain logic.

**Crates:**

- **auth** — reusable cryptographic infrastructure for password hashing and JWT generation and validation, shared across services without domain coupling.
- **web** — shared HTTP infrastructure: CORS layer, health/readiness checks, Prometheus metrics middleware, JWT auth middleware, request tracing. Used by both services' inbound HTTP layers.
- **outbox** — transactional outbox: aggregate writes and their events commit in one Postgres transaction, a polling relay publishes pending rows to the broker afterward. Payloads are opaque JSON; each service keeps its own wire format.

**Services:**

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
- `src/lib/inbound/` — Drivers
- `src/lib/outbound/` — Adapters
- `migrations/` — Database migrations

**Storage:**

- PostgreSQL — Users, channels, user replicas (read model)
- Cassandra — Messages (time-series, partitioned by channel_id)
- Kafka — `user-events`, `chat.messages` (48 partitions)

**Event Topics:**

*user-events (published by user-service)*
- `UserCreated` → {event_id, user_id, username, email, created_at}
- `UserUpdated` → {event_id, user_id, username, email, updated_at}
- `UserDeleted` → {event_id, user_id, deleted_at}

*chat.messages (published by chat-service)*
- `MessageSent` → {event_id, message_id, channel_id, user_id, content, timestamp}
- `MessageDeleted` → {event_id, message_id, channel_id, deleted_at}

**Eventual Consistency Model:**

chat-service maintains a denormalized `user_replica` table for fast username lookups:
- Populated via Kafka consumer from `user-events` topic
- Upserted on UserCreated/UserUpdated events
- Deleted on UserDeleted events
- Enables message enrichment with username data on read path
- gRPC fallback available for cache misses (user not yet in replica)

For detailed interaction flows, see the [sequence diagrams](./docs/sequence).

**Practices:** 

- Use type system and newtypes
- Use `thiserror` for domain errors
- Use `anyhow` for application errors
- Never `unwrap()` or `expect()` in production code
- Propagate with `?` operator
- User-facing error messages

## API

See [user-service](./services/user-service/openapi/user-service.yaml) and [chat-service](./services/chat-service/openapi/chat-service.yaml) OpenAPI specs for complete specifications.

## Kubernetes

A local `kind` cluster, its operators installed from pinned upstream Helm charts, and two repo-owned charts (`chat-infra` for Kafka, Postgres and Scylla CRs, `chat` for the app workloads) — see `deploy/operators/` for the full operator list.

```bash
# Bring up dev end to end, deployed via Argo CD from the pushed main branch.
# --direct installs from the working tree instead.
./scripts/k8s/dev-up.sh [--direct]

# End-to-end smoke test against dev (register, chat, verify delivery)
./scripts/k8s/dev-verify.sh

# Bring up staging: second namespace on the same cluster, dev-shaped infra,
# real HPA/PDB.
./scripts/k8s/staging-up.sh

# Rolling-restart proof: background load + WS reconnect monitor through
# Ingress, zero dropped requests, <5s reconnect gap
./scripts/k8s/staging-verify.sh

# Tear down
./scripts/k8s/dev-down.sh
./scripts/k8s/staging-down.sh
```

`deploy/`:
- `operators/` — pinned upstream Helm chart values (consumer-Helm)
- `charts/` — repo-owned Helm charts for infra CRs and app workloads (author-Helm)
- `cluster/` — cluster-scoped plain manifests (e.g. the self-signed `ClusterIssuer`) that can't live in a chart installed once per namespace
- `argocd/` — GitOps `Application` manifests, one per env for infra and one for app, and the cluster-issuer
- `kind/` — local cluster config

### Dashboards

All three are reachable only via port-forward, no public endpoint:

| Dashboard | URL | Login |
|---|---|---|
| Argo CD | http://localhost:8080 | user `admin`, password below |
| Grafana | http://localhost:3000 | user `admin`, password below |
| Headlamp | http://localhost:4466 | token below, no default password |

```bash
# Argo CD — sync status
kubectl -n argocd port-forward svc/argocd-server 8080:80
kubectl -n argocd get secret argocd-initial-admin-secret -o jsonpath='{.data.password}' | base64 -d

# Grafana — metrics
kubectl -n monitoring port-forward svc/kube-prometheus-stack-grafana 3000:80
kubectl -n monitoring get secret kube-prometheus-stack-grafana -o jsonpath='{.data.admin-password}' | base64 -d

# Headlamp — general cluster dashboard
kubectl -n headlamp port-forward svc/headlamp 4466:80
kubectl -n headlamp create token headlamp
```

## Testing
```bash
./scripts/tests/test.sh       # Full integration tests
cargo test --all       # With infrastructure running
```

## License
[MIT License](./LICENSE)
