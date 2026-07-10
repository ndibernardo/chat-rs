use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use web::health::HealthState;
use web::health::PgReadyCheck;
use web::health::PgSchemaReadyCheck;
use web::health::ReadyCheck;
use web::health::health_router;

use super::common;
use super::health_checks::ProducerReadyCheck;
use super::health_checks::ScyllaSchemaReadyCheck;
use crate::config::Config;
use crate::inbound::kafka::user_consumer::UserEventsConsumer;
use crate::outbound::kafka::EventProducer;
use outbox::OutboxRelay;

/// `chat-worker`: background consumers and the outbox relay (persister and
/// deleted-user cleanup join later). Serves a health-only HTTP listener —
/// no API routes, no WebSocket route.
pub async fn run(config: Config) -> Result<(), anyhow::Error> {
    tracing::info!(
        cassandra_nodes = ?config.cassandra.nodes,
        kafka_brokers = %config.kafka.brokers,
        "Configuration loaded"
    );
    web::metrics::install_prometheus_recorder(config.server.metrics_port)?;

    let pg_pool = common::connect_pg_pool(&config).await?;

    let user_repository = Arc::new(crate::outbound::postgres::UserReplicaRepository::new(
        pg_pool.clone(),
    ));

    let user_events_consumer = UserEventsConsumer::new(&config, user_repository)?;
    let event_producer = Arc::new(EventProducer::new(&config)?);

    let checks: Vec<Arc<dyn ReadyCheck>> = vec![
        Arc::new(PgReadyCheck::new(pg_pool.clone())),
        Arc::new(PgSchemaReadyCheck::new(
            pg_pool.clone(),
            sqlx::migrate!("./migrations"),
        )),
        Arc::new(ScyllaSchemaReadyCheck::new(config.cassandra.clone())),
        Arc::new(user_events_consumer.assignment_tracker()),
        Arc::new(ProducerReadyCheck::new(Arc::clone(&event_producer))),
    ];

    let consumer_cancellation = CancellationToken::new();
    let user_consumer_token = consumer_cancellation.clone();

    tracing::info!(
        consumer = "user_events",
        topic = %config.kafka.user_events.topic,
        "Starting Kafka user event consumer"
    );
    let user_consumer_handle = tokio::spawn(async move {
        user_events_consumer
            .start_consuming(user_consumer_token)
            .await;
    });

    let relay = OutboxRelay::new(
        pg_pool.clone(),
        Arc::clone(&event_producer),
        config.outbox.clone(),
    );
    let relay_cancellation = CancellationToken::new();
    let relay_token = relay_cancellation.clone();
    tracing::info!("Starting outbox relay");
    let relay_handle = tokio::spawn(async move {
        relay.run(relay_token).await;
    });

    let health_state = HealthState::new(checks);
    let draining = health_state.draining_flag();
    let application = health_router(health_state);

    let http_address = format!("0.0.0.0:{}", config.server.http_port);
    let listener = tokio::net::TcpListener::bind(&http_address).await?;
    tracing::info!(
        address = %http_address,
        port = config.server.http_port,
        role = "worker",
        "Health listener serving"
    );

    axum::serve(listener, application)
        .with_graceful_shutdown(common::graceful_shutdown(
            draining,
            Duration::from_secs(config.shutdown.readiness_delay_seconds),
        ))
        .await?;

    tracing::info!("HTTP server stopped, shutting down Kafka consumer and outbox relay");
    common::stop_consumer("user_events", &consumer_cancellation, user_consumer_handle).await;
    common::stop_consumer("outbox_relay", &relay_cancellation, relay_handle).await;

    Ok(())
}
