use std::sync::Arc;
use std::time::Duration;

use web::health::HealthState;
use web::health::PgReadyCheck;
use web::health::PgSchemaReadyCheck;
use web::health::ReadyCheck;
use web::health::health_router;

use super::common;
use crate::config::Config;

/// `user-service --role outbox-relay`: drains the Postgres outbox table into
/// Kafka. The relay loop itself lands separately; until then this role
/// serves a health-only HTTP listener so the Deployment/health-probe shape
/// is already in place.
pub async fn run(config: Config) -> Result<(), anyhow::Error> {
    web::metrics::install_prometheus_recorder(config.server.metrics_port)?;

    let pg_pool = common::connect_pg_pool(&config).await?;

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

    Ok(())
}
