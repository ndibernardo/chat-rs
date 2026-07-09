use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::config::Config;

pub async fn connect_pg_pool(config: &Config) -> Result<PgPool, anyhow::Error> {
    let pg_pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await?;
    tracing::info!(
        max_connections = config.database.max_connections,
        database = "postgresql",
        "Database connection pool created"
    );
    Ok(pg_pool)
}

/// `--migrate-only`: apply pending Postgres schema changes, then return.
/// The Kubernetes Job entrypoint; normal server boot never calls this — it
/// only asserts the schema is already current (see `web::health::PgSchemaReadyCheck`).
pub async fn migrate_only(config: Config) -> Result<(), anyhow::Error> {
    let pg_pool = connect_pg_pool(&config).await?;
    sqlx::migrate!("./migrations").run(&pg_pool).await?;
    tracing::info!(database = "postgresql", "Database migrations completed");
    Ok(())
}

/// Resolves on SIGINT or SIGTERM. Each server task calls this independently;
/// tokio's signal listeners broadcast to every registered receiver of the
/// same kind.
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

/// On SIGINT/SIGTERM, marks `/readyz` as draining and waits `readiness_delay`
/// before returning, giving the load balancer/endpoint controller time to
/// notice and stop routing new traffic here before this process actually
/// stops accepting connections.
pub async fn graceful_shutdown(draining: Arc<AtomicBool>, readiness_delay: Duration) {
    wait_for_shutdown_signal().await;

    tracing::info!("Shutdown signal received, marking not ready");
    draining.store(true, Ordering::Relaxed);
    tokio::time::sleep(readiness_delay).await;

    tracing::info!("Starting graceful shutdown");
}

/// Strip `user:password@` credentials from a connection URL before logging it.
pub fn redact_credentials(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let after_scheme = &url[scheme_end + 3..];
    match after_scheme.find('@') {
        Some(at_pos) => format!(
            "{}://***@{}",
            &url[..scheme_end],
            &after_scheme[at_pos + 1..]
        ),
        None => url.to_string(),
    }
}
