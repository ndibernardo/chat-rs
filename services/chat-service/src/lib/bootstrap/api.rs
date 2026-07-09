use std::sync::Arc;

use web::health::HealthState;
use web::health::PgReadyCheck;
use web::health::PgSchemaReadyCheck;
use web::health::ReadyCheck;
use web::health::health_router;

use super::common;
use super::health_checks::ScyllaReadyCheck;
use super::health_checks::ScyllaSchemaReadyCheck;
use crate::config::Config;
use crate::inbound::http::api_routes;
use crate::inbound::http::build_router;
use crate::inbound::http::router::AppState;

/// `chat-api`: channel API and message history over HTTP. No Kafka consumers,
/// no WebSocket route.
pub async fn run_api_only(config: Config) -> Result<(), anyhow::Error> {
    log_config(&config);
    web::metrics::install_prometheus_recorder(config.server.metrics_port)?;

    let pg_pool = common::connect_pg_pool(&config).await?;
    let adapters = common::build_adapters(&config, pg_pool).await?;

    let checks: Vec<Arc<dyn ReadyCheck>> = vec![
        Arc::new(PgReadyCheck::new(adapters.pg_pool.clone())),
        Arc::new(PgSchemaReadyCheck::new(
            adapters.pg_pool.clone(),
            sqlx::migrate!("./migrations"),
        )),
        Arc::new(ScyllaReadyCheck::new(Arc::clone(
            &adapters.message_repository,
        ))),
        Arc::new(ScyllaSchemaReadyCheck::new(config.cassandra.clone())),
    ];

    let state = AppState {
        channel_service: adapters.channel_service,
        message_service: adapters.message_service,
        connection_registry: adapters.connection_registry,
        authenticator: adapters.authenticator.clone(),
        ws_send_queue_capacity: config.websocket.send_queue_capacity,
    };

    let routes = api_routes(adapters.authenticator);
    let application = build_router(routes, state).merge(health_router(HealthState::new(checks)));

    serve_http(&config, application).await
}

/// `all`: today's single-binary dev default — API, WS gateway and both
/// consumers in one process.
pub async fn run_all(config: Config) -> Result<(), anyhow::Error> {
    log_config(&config);
    web::metrics::install_prometheus_recorder(config.server.metrics_port)?;

    let pg_pool = common::connect_pg_pool(&config).await?;
    let adapters = common::build_adapters(&config, pg_pool).await?;

    let message_event_consumer = crate::inbound::kafka::consumer::EventConsumer::new(
        &config,
        adapters.connection_registry.clone()
            as std::sync::Arc<dyn crate::domain::message::ports::MessageBroadcaster>,
    )?;
    let user_events_consumer = crate::inbound::kafka::user_consumer::UserEventsConsumer::new(
        &config,
        adapters.user_repository.clone(),
    )?;

    let checks: Vec<Arc<dyn ReadyCheck>> = vec![
        Arc::new(PgReadyCheck::new(adapters.pg_pool.clone())),
        Arc::new(PgSchemaReadyCheck::new(
            adapters.pg_pool.clone(),
            sqlx::migrate!("./migrations"),
        )),
        Arc::new(ScyllaReadyCheck::new(Arc::clone(
            &adapters.message_repository,
        ))),
        Arc::new(ScyllaSchemaReadyCheck::new(config.cassandra.clone())),
        Arc::new(super::health_checks::ProducerReadyCheck::new(Arc::clone(
            &adapters.event_producer,
        ))),
        Arc::new(message_event_consumer.assignment_tracker()),
        Arc::new(user_events_consumer.assignment_tracker()),
    ];

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

    let application = crate::inbound::http::create_router(
        adapters.channel_service,
        adapters.message_service,
        adapters.connection_registry,
        adapters.authenticator,
        config.websocket.send_queue_capacity,
    )
    .merge(health_router(HealthState::new(checks)));

    serve_http(&config, application).await?;

    tracing::info!("HTTP server stopped, shutting down Kafka consumers");
    message_consumer_handle.abort();
    user_consumer_handle.abort();

    Ok(())
}

async fn serve_http(config: &Config, application: axum::Router) -> Result<(), anyhow::Error> {
    let http_address = format!("0.0.0.0:{}", config.server.http_port);
    let listener = tokio::net::TcpListener::bind(&http_address).await?;
    tracing::info!(
        address = %http_address,
        port = config.server.http_port,
        protocols = "http,websocket",
        "Server Listening"
    );

    axum::serve(listener, application)
        .with_graceful_shutdown(common::shutdown_signal())
        .await?;

    Ok(())
}

fn log_config(config: &Config) {
    tracing::info!(
        database_url = %common::redact_credentials(&config.database.url),
        cassandra_nodes = ?config.cassandra.nodes,
        cassandra_keyspace = %config.cassandra.keyspace,
        http_port = config.server.http_port,
        user_service_grpc_url = %config.user_service.grpc_url,
        kafka_brokers = %config.kafka.brokers,
        kafka_group_id = %config.kafka.group_id,
        kafka_messages_topic = %config.kafka.messages_topic,
        "Configuration loaded"
    );
}
