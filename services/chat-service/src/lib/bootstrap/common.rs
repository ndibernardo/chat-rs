use std::sync::Arc;

use auth::Authenticator;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

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
    pub channel_service:
        Arc<ChannelService<postgres::ChannelRepository, kafka::ChannelEventPublisher>>,
    pub message_service: Arc<
        MessageService<
            scylla::MessageRepository,
            ReplicaWithFallback<postgres::UserReplicaRepository, grpc::UserServiceClient>,
            kafka::MessageEventPublisher,
        >,
    >,
    pub user_repository: Arc<postgres::UserReplicaRepository>,
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

pub async fn run_pg_migrations(pg_pool: &PgPool) -> Result<(), anyhow::Error> {
    sqlx::migrate!("./migrations").run(pg_pool).await?;
    tracing::info!(database = "postgresql", "Database migrations completed");
    Ok(())
}

pub async fn run_scylla_migrations(config: &Config) -> Result<(), anyhow::Error> {
    scylla::migrations::run(&config.cassandra).await?;
    tracing::info!(database = "cassandra", "Cassandra migrations completed");
    Ok(())
}

/// Build every adapter the `all` role needs. Split roles construct the
/// subset relevant to them directly rather than paying for unused adapters
/// (e.g. `chat-api` has no reason to hold a gRPC channel to itself via a
/// consumer it never starts).
pub async fn build_adapters(config: &Config, pg_pool: PgPool) -> Result<Adapters, anyhow::Error> {
    let authenticator = Arc::new(Authenticator::new(config.jwt.secret.as_bytes()));
    let connection_registry = Arc::new(ConnectionRegistry::new());

    let channel_repository = Arc::new(postgres::ChannelRepository::new(pg_pool.clone()));
    let message_repository = Arc::new(scylla::MessageRepository::new(config).await?);
    let user_repository = Arc::new(postgres::UserReplicaRepository::new(pg_pool));

    let event_producer = Arc::new(kafka::EventProducer::new(config)?);
    let channel_event_publisher = Arc::new(kafka::ChannelEventPublisher::new(Arc::clone(
        &event_producer,
    )));
    let message_event_publisher = Arc::new(kafka::MessageEventPublisher::new(Arc::clone(
        &event_producer,
    )));

    let channel_service = Arc::new(ChannelService::new(
        channel_repository,
        channel_event_publisher,
    ));

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
        message_repository,
        user_resolver,
        message_event_publisher,
    ));

    Ok(Adapters {
        authenticator,
        connection_registry,
        channel_service,
        message_service,
        user_repository,
    })
}

/// Resolves on SIGINT or SIGTERM, so servers can stop accepting new
/// connections and drain in-flight requests instead of the process being
/// killed mid-request. Each server task calls this independently; tokio's
/// signal listeners broadcast to every registered receiver of the same kind.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, starting graceful shutdown");
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
