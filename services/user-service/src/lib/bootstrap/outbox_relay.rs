use super::common;
use crate::config::Config;
use crate::inbound::http::handlers::health::health;

/// `user-service --role outbox-relay`: drains the Postgres outbox table into
/// Kafka. The relay loop itself lands separately; until then this role
/// serves a health-only HTTP listener so the Deployment/health-probe shape
/// is already in place.
pub async fn run(config: Config) -> Result<(), anyhow::Error> {
    let pg_pool = common::connect_pg_pool(&config).await?;
    common::run_pg_migrations(&pg_pool).await?;

    let health_router = axum::Router::new().route("/health", axum::routing::get(health));

    let http_address = format!("0.0.0.0:{}", config.server.http_port);
    let listener = tokio::net::TcpListener::bind(&http_address).await?;
    tracing::info!(
        address = %http_address,
        port = config.server.http_port,
        role = "outbox-relay",
        "Health listener serving"
    );

    axum::serve(listener, health_router)
        .with_graceful_shutdown(common::shutdown_signal())
        .await?;

    Ok(())
}
