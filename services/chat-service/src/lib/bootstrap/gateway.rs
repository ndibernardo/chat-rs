use std::sync::Arc;

use crate::config::Config;
use crate::domain::message::ports::MessageBroadcaster;
use crate::inbound::http::build_router;
use crate::inbound::http::health_routes;
use crate::inbound::http::router::AppState;
use crate::inbound::http::ws_routes;
use crate::inbound::kafka::consumer::EventConsumer;

use super::common;

/// `chat-ws-gateway`: WebSocket upgrades + broadcast fan-out + producer. No
/// API routes, no user-replica consumer.
pub async fn run(config: Config) -> Result<(), anyhow::Error> {
    tracing::info!(
        cassandra_nodes = ?config.cassandra.nodes,
        http_port = config.server.http_port,
        kafka_brokers = %config.kafka.brokers,
        kafka_group_id = %config.kafka.group_id,
        "Configuration loaded"
    );

    let pg_pool = common::connect_pg_pool(&config).await?;
    common::run_pg_migrations(&pg_pool).await?;
    common::run_scylla_migrations(&config).await?;

    let adapters = common::build_adapters(&config, pg_pool).await?;

    let message_event_consumer = EventConsumer::new(
        &config,
        adapters.connection_registry.clone() as Arc<dyn MessageBroadcaster>,
    )?;

    tracing::info!(
        consumer = "message_events",
        topics = "chat.messages.*",
        "Starting Kafka message event consumer"
    );
    let message_consumer_handle = tokio::spawn(async move {
        message_event_consumer.start_consuming().await;
    });

    let state = AppState {
        channel_service: adapters.channel_service,
        message_service: adapters.message_service,
        connection_registry: adapters.connection_registry,
        authenticator: adapters.authenticator,
    };

    let routes = health_routes().merge(ws_routes());
    let application = build_router(routes, state);

    let http_address = format!("0.0.0.0:{}", config.server.http_port);
    let listener = tokio::net::TcpListener::bind(&http_address).await?;
    tracing::info!(
        address = %http_address,
        port = config.server.http_port,
        protocols = "http,websocket",
        role = "gateway",
        "Server Listening"
    );

    axum::serve(listener, application)
        .with_graceful_shutdown(common::shutdown_signal())
        .await?;

    tracing::info!("HTTP server stopped, shutting down Kafka consumer");
    message_consumer_handle.abort();

    Ok(())
}
