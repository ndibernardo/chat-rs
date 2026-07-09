use crate::config::Config;
use crate::inbound::kafka::user_consumer::UserEventsConsumer;

use super::common;

/// `chat-worker`: background consumers only (user-replica today; persister,
/// outbox-relay, and deleted-user cleanup join later). Serves a
/// health-only HTTP listener — no API routes, no WebSocket route.
pub async fn run(config: Config) -> Result<(), anyhow::Error> {
    tracing::info!(
        cassandra_nodes = ?config.cassandra.nodes,
        kafka_brokers = %config.kafka.brokers,
        "Configuration loaded"
    );

    let pg_pool = common::connect_pg_pool(&config).await?;
    common::run_pg_migrations(&pg_pool).await?;
    common::run_scylla_migrations(&config).await?;

    let user_repository = std::sync::Arc::new(
        crate::outbound::postgres::UserReplicaRepository::new(pg_pool),
    );

    let user_events_consumer = UserEventsConsumer::new(&config, user_repository)?;

    tracing::info!(
        consumer = "user_events",
        topic = %config.kafka.user_events.topic,
        "Starting Kafka user event consumer"
    );
    let user_consumer_handle = tokio::spawn(async move {
        user_events_consumer.start_consuming().await;
    });

    let health_router = axum::Router::new().route(
        "/health",
        axum::routing::get(crate::inbound::http::handlers::health::health),
    );

    let http_address = format!("0.0.0.0:{}", config.server.http_port);
    let listener = tokio::net::TcpListener::bind(&http_address).await?;
    tracing::info!(
        address = %http_address,
        port = config.server.http_port,
        role = "worker",
        "Health listener serving"
    );

    axum::serve(listener, health_router)
        .with_graceful_shutdown(common::shutdown_signal())
        .await?;

    tracing::info!("HTTP server stopped, shutting down Kafka consumer");
    user_consumer_handle.abort();

    Ok(())
}
