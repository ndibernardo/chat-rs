use std::sync::Arc;

use anyhow::Error;
use auth::Authenticator;
use chat_service::config::Config;
use chat_service::domain::channel::service::Service as ChannelService;
use chat_service::domain::message::service::Service as MessageService;
use chat_service::inbound::http::create_router;
use chat_service::inbound::websocket::registry::ConnectionRegistry;
use chat_service::outbound::grpc::user::UserServiceClient;
use chat_service::outbound::kafka::channel_publisher::ChannelEventPublisher;
use chat_service::outbound::kafka::consumer::EventConsumer;
use chat_service::outbound::kafka::message_publisher::MessageEventPublisher;
use chat_service::outbound::kafka::producer::EventProducer;
use chat_service::outbound::kafka::user_consumer::UserEventsConsumer;
use chat_service::outbound::postgres::channel::ChannelRepository;
use chat_service::outbound::postgres::user_replica::UserReplicaRepository;
use chat_service::outbound::resolver::ReplicaWithFallback;
use chat_service::outbound::scylla::message::MessageRepository;
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
        database_url = %config.database.url,
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

    let channel_repository = Arc::new(ChannelRepository::new(pg_pool.clone()));
    let message_repository = Arc::new(MessageRepository::new(&config).await?);
    let user_repository = Arc::new(UserReplicaRepository::new(pg_pool));

    let event_producer = Arc::new(EventProducer::new(&config)?);
    let message_event_consumer =
        EventConsumer::new(&config, Arc::clone(&connection_registry))?;
    let user_events_consumer = UserEventsConsumer::new(&config, Arc::clone(&user_repository))?;
    let channel_event_publisher =
        Arc::new(ChannelEventPublisher::new(Arc::clone(&event_producer)));
    let message_event_publisher =
        Arc::new(MessageEventPublisher::new(Arc::clone(&event_producer)));

    let channel_service = Arc::new(ChannelService::new(
        channel_repository,
        channel_event_publisher,
    ));

    let grpc_user_client = Arc::new(
        UserServiceClient::new(&config.user_service.grpc_url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to user-service: {}", e))?,
    );
    let user_resolver = Arc::new(ReplicaWithFallback::new(user_repository, grpc_user_client));

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
    tokio::spawn(async move {
        message_event_consumer.start_consuming().await;
    });

    tracing::info!(
        consumer = "user_events",
        topic = %config.kafka.user_events.topic,
        "Starting Kafka user event consumer"
    );
    tokio::spawn(async move {
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

    axum::serve(listener, application).await?;

    Ok(())
}
