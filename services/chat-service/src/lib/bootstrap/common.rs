use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use auth::Authenticator;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::domain::channel::service::Service as ChannelService;
use crate::domain::message::service::Service as MessageService;
use crate::inbound::websocket::registry::ConnectionRegistry;
use crate::outbound::grpc;
use crate::outbound::kafka;
use crate::outbound::postgres;
use crate::outbound::resolver::ReplicaWithFallback;
use crate::outbound::scylla;

/// Shared adapter set built once and composed differently by each role
/// runner. Building everything up front keeps the constructors in one
/// place; each `bootstrap::*::run` picks only the fields its role needs.
pub struct Adapters {
    pub authenticator: Arc<Authenticator>,
    pub connection_registry: Arc<ConnectionRegistry>,
    pub channel_service: Arc<ChannelService<postgres::ChannelRepository>>,
    pub message_service: Arc<
        MessageService<
            scylla::MessageRepository,
            ReplicaWithFallback<postgres::UserReplicaRepository, grpc::UserServiceClient>,
            kafka::MessageEventPublisher,
        >,
    >,
    pub user_repository: Arc<postgres::UserReplicaRepository>,
    /// Kept alongside `channel_service` (which owns its own handle) so the
    /// deleted-user cleanup consumer can erase memberships and deactivate
    /// direct channels without going through the domain service API.
    pub channel_repository: Arc<postgres::ChannelRepository>,
    /// Kept alongside `message_service` (which owns its own handle) so
    /// readiness checks can ping Scylla without reaching through the
    /// message service's domain-level API.
    pub message_repository: Arc<scylla::MessageRepository>,
    pub event_producer: Arc<kafka::EventProducer>,
    /// A spare handle to the pool passed in, for readiness checks — the
    /// repositories built from it each hold their own clone already.
    pub pg_pool: PgPool,
}

pub async fn connect_pg_pool(config: &Config) -> Result<PgPool, anyhow::Error> {
    let pg_pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await?;
    tracing::info!(
        max_connections = config.database.max_connections,
        database = "postgresql",
        "Database connection pool created"
    );
    Ok(pg_pool)
}

/// `--migrate-only`: apply pending Postgres + Scylla schema changes, then
/// return. The Kubernetes Job entrypoint; normal server boot never calls
/// this — it only asserts the schema is already current (see
/// `web::health::PgSchemaReadyCheck` and `scylla::migrations::check_schema`).
pub async fn migrate_only(config: Config) -> Result<(), anyhow::Error> {
    let pg_pool = connect_pg_pool(&config).await?;
    sqlx::migrate!("./migrations").run(&pg_pool).await?;
    tracing::info!(database = "postgresql", "Database migrations completed");

    scylla::migrations::run(&config.cassandra).await?;
    tracing::info!(database = "cassandra", "Cassandra migrations completed");

    Ok(())
}

/// Build every adapter the `all` role needs. Split roles construct the
/// subset relevant to them directly rather than paying for unused adapters
/// (e.g. `chat-api` has no reason to hold a gRPC channel to itself via a
/// consumer it never starts).
pub async fn build_adapters(config: &Config, pg_pool: PgPool) -> Result<Adapters, anyhow::Error> {
    let public_key_pem = std::fs::read(&config.jwt.public_key_path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to read JWT public key at {}: {e}",
            config.jwt.public_key_path
        )
    })?;
    let authenticator = Arc::new(
        Authenticator::verifier(&public_key_pem)
            .map_err(|e| anyhow::anyhow!("Failed to build JWT verifier: {e}"))?,
    );
    let connection_registry = Arc::new(ConnectionRegistry::new());

    let channel_repository = Arc::new(postgres::ChannelRepository::new(
        pg_pool.clone(),
        config.kafka.messages_topic.clone(),
    ));
    let message_repository = Arc::new(scylla::MessageRepository::new(config).await?);
    let user_repository = Arc::new(postgres::UserReplicaRepository::new(pg_pool.clone()));

    let event_producer = Arc::new(kafka::EventProducer::new(config)?);
    let message_event_publisher = Arc::new(kafka::MessageEventPublisher::new(Arc::clone(
        &event_producer,
    )));

    let channel_service = Arc::new(ChannelService::new(Arc::clone(&channel_repository)));

    let grpc_user_client = Arc::new(
        grpc::UserServiceClient::new(&config.user_service.grpc_url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to build user-service gRPC client: {}", e))?,
    );
    let user_resolver = Arc::new(ReplicaWithFallback::new(
        Arc::clone(&user_repository),
        grpc_user_client,
    ));

    let message_service = Arc::new(MessageService::new(
        Arc::clone(&message_repository),
        user_resolver,
        message_event_publisher,
    ));

    Ok(Adapters {
        authenticator,
        connection_registry,
        channel_service,
        message_service,
        user_repository,
        channel_repository,
        message_repository,
        event_producer,
        pg_pool,
    })
}

/// Resolves on SIGINT or SIGTERM. Each server task calls this independently;
/// tokio's signal listeners broadcast to every registered receiver of the
/// same kind.
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

/// Graceful shutdown for roles with no WebSocket connections to drain: on
/// SIGINT/SIGTERM, marks `/readyz` as draining and waits `readiness_delay`
/// before returning, giving the load balancer/endpoint controller time to
/// notice and stop routing new traffic here before this process actually
/// stops accepting connections.
pub async fn graceful_shutdown(draining: Arc<AtomicBool>, readiness_delay: Duration) {
    wait_for_shutdown_signal().await;

    tracing::info!("Shutdown signal received, marking not ready");
    draining.store(true, Ordering::Relaxed);
    tokio::time::sleep(readiness_delay).await;

    tracing::info!("Starting graceful shutdown");
}

/// Graceful shutdown for roles serving WebSocket connections: same sequence
/// as [`graceful_shutdown`], then sends every connection a close frame and
/// waits up to `drain_grace` for clients to disconnect.
///
/// Bounded by `drain_grace` — a client that never closes doesn't block this
/// function forever, though axum's own connection drain (`axum::serve`'s
/// graceful shutdown) may still wait briefly afterward for the underlying
/// socket to finish closing.
pub async fn graceful_ws_shutdown(
    draining: Arc<AtomicBool>,
    connection_registry: Arc<ConnectionRegistry>,
    readiness_delay: Duration,
    drain_grace: Duration,
) {
    wait_for_shutdown_signal().await;

    tracing::info!("Shutdown signal received, marking not ready");
    draining.store(true, Ordering::Relaxed);
    tokio::time::sleep(readiness_delay).await;

    tracing::info!("Closing active WebSocket connections");
    connection_registry.close_all().await;

    let deadline = tokio::time::Instant::now() + drain_grace;
    loop {
        let remaining = connection_registry.get_total_connections().await;
        if remaining == 0 {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            tracing::warn!(
                remaining,
                "Drain grace period elapsed with connections still open"
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    tracing::info!("Starting graceful shutdown");
}

/// Upper bound on how long a cancelled consumer task gets to actually stop,
/// so a stuck consumer can't block process exit indefinitely.
const CONSUMER_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Cancels a consumer's cooperative-shutdown token and waits for its task to
/// finish, bounded by [`CONSUMER_JOIN_TIMEOUT`].
pub async fn stop_consumer(name: &'static str, token: &CancellationToken, handle: JoinHandle<()>) {
    token.cancel();
    if tokio::time::timeout(CONSUMER_JOIN_TIMEOUT, handle)
        .await
        .is_err()
    {
        tracing::warn!(
            consumer = name,
            "Consumer task did not stop within the shutdown timeout"
        );
    }
}

/// Strip `user:password@` credentials from a connection URL before logging it.
pub fn redact_credentials(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let after_scheme = &url[scheme_end + 3..];
    match after_scheme.find('@') {
        Some(at_pos) => format!(
            "{}://***@{}",
            &url[..scheme_end],
            &after_scheme[at_pos + 1..]
        ),
        None => url.to_string(),
    }
}
