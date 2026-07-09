use std::sync::Arc;

use web::health::HealthState;
use web::health::PgReadyCheck;
use web::health::PgSchemaReadyCheck;
use web::health::ReadyCheck;
use web::health::health_router;

use super::common;
use super::health_checks::ScyllaSchemaReadyCheck;
use crate::config::Config;
use crate::inbound::kafka::user_consumer::UserEventsConsumer;

/// `chat-worker`: background consumers only (user-replica today; persister,
/// outbox-relay, and deleted-user cleanup join later). Serves a
/// health-only HTTP listener — no API routes, no WebSocket route.
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

    let checks: Vec<Arc<dyn ReadyCheck>> = vec![
        Arc::new(PgReadyCheck::new(pg_pool.clone())),
        Arc::new(PgSchemaReadyCheck::new(
            pg_pool,
            sqlx::migrate!("./migrations"),
        )),
        Arc::new(ScyllaSchemaReadyCheck::new(config.cassandra.clone())),
        Arc::new(user_events_consumer.assignment_tracker()),
    ];

    tracing::info!(
        consumer = "user_events",
        topic = %config.kafka.user_events.topic,
        "Starting Kafka user event consumer"
    );
    let user_consumer_handle = tokio::spawn(async move {
        user_events_consumer.start_consuming().await;
    });

    let application = health_router(HealthState::new(checks));

    let http_address = format!("0.0.0.0:{}", config.server.http_port);
    let listener = tokio::net::TcpListener::bind(&http_address).await?;
    tracing::info!(
        address = %http_address,
        port = config.server.http_port,
        role = "worker",
        "Health listener serving"
    );

    axum::serve(listener, application)
        .with_graceful_shutdown(common::shutdown_signal())
        .await?;

    tracing::info!("HTTP server stopped, shutting down Kafka consumer");
    user_consumer_handle.abort();

    Ok(())
}
