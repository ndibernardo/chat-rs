use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use web::health::HealthState;
use web::health::PgReadyCheck;
use web::health::PgSchemaReadyCheck;
use web::health::ReadyCheck;
use web::health::health_router;

use super::common;
use crate::config::Config;
use crate::outbound::kafka::EventProducer;
use outbox::OutboxRelay;

/// `user-service --role outbox-relay`: drains the Postgres outbox table into
/// Kafka, alongside a health-only HTTP listener.
pub async fn run(config: Config) -> Result<(), anyhow::Error> {
    web::metrics::install_prometheus_recorder(config.server.metrics_port)?;

    let pg_pool = common::connect_pg_pool(&config).await?;
    let event_producer = Arc::new(EventProducer::new(&config)?);

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

    let checks: Vec<Arc<dyn ReadyCheck>> = vec![
        Arc::new(PgReadyCheck::new(pg_pool.clone())),
        Arc::new(PgSchemaReadyCheck::new(
            pg_pool,
            sqlx::migrate!("./migrations"),
        )),
    ];
    let health_state = HealthState::new(checks);
    let draining = health_state.draining_flag();
    let application = health_router(health_state);

    let http_address = format!("0.0.0.0:{}", config.server.http_port);
    let listener = tokio::net::TcpListener::bind(&http_address).await?;
    tracing::info!(
        address = %http_address,
        port = config.server.http_port,
        role = "outbox-relay",
        "Health listener serving"
    );

    axum::serve(listener, application)
        .with_graceful_shutdown(common::graceful_shutdown(
            draining,
            Duration::from_secs(config.shutdown.readiness_delay_seconds),
        ))
        .await?;

    tracing::info!("HTTP server stopped, shutting down outbox relay");
    common::stop_consumer("outbox_relay", &relay_cancellation, relay_handle).await;

    Ok(())
}
