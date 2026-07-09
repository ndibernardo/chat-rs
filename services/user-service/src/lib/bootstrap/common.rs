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
/// only asserts the schema is already current (see `web::PgSchemaReadyCheck`).
pub async fn migrate_only(config: Config) -> Result<(), anyhow::Error> {
    let pg_pool = connect_pg_pool(&config).await?;
    sqlx::migrate!("./migrations").run(&pg_pool).await?;
    tracing::info!(database = "postgresql", "Database migrations completed");
    Ok(())
}

/// Resolves on SIGINT or SIGTERM, so servers can stop accepting new
/// connections and drain in-flight requests instead of the process being
/// killed mid-request. Each server task calls this independently; tokio's
/// signal listeners broadcast to every registered receiver of the same kind.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, starting graceful shutdown");
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
