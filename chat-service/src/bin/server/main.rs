use std::sync::Arc;

use anyhow::Error;
use auth::Authenticator;
use chat_service::config::Config;
use chat_service::domain::channel::service::Service as ChannelService;
use chat_service::domain::message::ports::MessageBroadcaster;
use chat_service::domain::message::service::Service as MessageService;
use chat_service::inbound::http::create_router;
use chat_service::inbound::kafka::consumer::EventConsumer;
use chat_service::inbound::kafka::user_consumer::UserEventsConsumer;
use chat_service::inbound::websocket::registry::ConnectionRegistry;
use chat_service::outbound::grpc;
use chat_service::outbound::kafka;
use chat_service::outbound::postgres;
use chat_service::outbound::resolver;
use chat_service::outbound::scylla;
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "chat_service=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!(
        service = "chat-service",
        version = env!("CARGO_PKG_VERSION"),
        "Service starting"
    );

    let config = Config::load()?;

    tracing::info!(
        database_url = %redact_credentials(&config.database.url),
        cassandra_nodes = ?config.cassandra.nodes,
        cassandra_keyspace = %config.cassandra.keyspace,
        http_port = config.server.http_port,
        user_service_grpc_url = %config.user_service.grpc_url,
        kafka_brokers = %config.kafka.brokers,
        kafka_group_id = %config.kafka.group_id,
        kafka_num_shards = config.kafka.num_shards,
        "Configuration loaded"
    );

    let pg_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database.url)
        .await?;
    tracing::info!(
        max_connections = 5,
        database = "postgresql",
        "Database connection pool created"
    );

    sqlx::migrate!("./migrations").run(&pg_pool).await?;
    tracing::info!(database = "postgresql", "Database migrations completed");

    let authenticator = Arc::new(Authenticator::new(config.jwt.secret.as_bytes()));
    let connection_registry = Arc::new(ConnectionRegistry::new());

    scylla::migrations::run(&config.cassandra).await?;
    tracing::info!(database = "cassandra", "Cassandra migrations completed");

    let channel_repository = Arc::new(postgres::ChannelRepository::new(pg_pool.clone()));
    let message_repository = Arc::new(scylla::MessageRepository::new(&config).await?);
    let user_repository = Arc::new(postgres::UserReplicaRepository::new(pg_pool));

    let event_producer = Arc::new(kafka::EventProducer::new(&config)?);
    let message_event_consumer = EventConsumer::new(
        &config,
        Arc::clone(&connection_registry) as Arc<dyn MessageBroadcaster>,
    )?;
    let user_events_consumer = UserEventsConsumer::new(&config, Arc::clone(&user_repository))?;
    let channel_event_publisher =
        Arc::new(kafka::ChannelEventPublisher::new(Arc::clone(&event_producer)));
    let message_event_publisher =
        Arc::new(kafka::MessageEventPublisher::new(Arc::clone(&event_producer)));

    let channel_service = Arc::new(ChannelService::new(
        channel_repository,
        channel_event_publisher,
    ));

    let grpc_user_client = Arc::new(
        grpc::UserServiceClient::new(&config.user_service.grpc_url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to build user-service gRPC client: {}", e))?,
    );
    let user_resolver = Arc::new(resolver::ReplicaWithFallback::new(
        user_repository,
        grpc_user_client,
    ));

    let message_service = Arc::new(MessageService::new(
        message_repository,
        user_resolver,
        message_event_publisher,
    ));

    tracing::info!(
        consumer = "message_events",
        topics = "chat.messages.*",
        "Starting Kafka message event consumer"
    );
    let message_consumer_handle = tokio::spawn(async move {
        message_event_consumer.start_consuming().await;
    });

    tracing::info!(
        consumer = "user_events",
        topic = %config.kafka.user_events.topic,
        "Starting Kafka user event consumer"
    );
    let user_consumer_handle = tokio::spawn(async move {
        user_events_consumer.start_consuming().await;
    });

    let http_address = format!("0.0.0.0:{}", config.server.http_port);
    let listener = tokio::net::TcpListener::bind(&http_address).await?;
    tracing::info!(
        address = %http_address,
        port = config.server.http_port,
        protocols = "http,websocket",
        "Server Listening"
    );

    let application = create_router(
        channel_service,
        message_service,
        connection_registry,
        authenticator,
    );

    axum::serve(listener, application)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("HTTP server stopped, shutting down Kafka consumers");
    message_consumer_handle.abort();
    user_consumer_handle.abort();

    Ok(())
}

/// Resolves on SIGINT or SIGTERM, so `axum::serve` can stop accepting new
/// connections and drain in-flight requests/WS connections instead of the
/// process being killed mid-request.
async fn shutdown_signal() {
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
fn redact_credentials(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let after_scheme = &url[scheme_end + 3..];
    match after_scheme.find('@') {
        Some(at_pos) => format!("{}://***@{}", &url[..scheme_end], &after_scheme[at_pos + 1..]),
        None => url.to_string(),
    }
}
