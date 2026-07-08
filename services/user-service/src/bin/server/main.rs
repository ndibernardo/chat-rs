use std::sync::Arc;

use auth::Authenticator;
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use user_service::config::Config;
use user_service::domain::user::service::Service as UserService;
use user_service::inbound::grpc::UserGrpcService;
use user_service::inbound::http::router::create_router;
use user_service::outbound::argon2::PasswordHasher;
use user_service::outbound::kafka::EventProducer;
use user_service::outbound::postgres::UserRepository;
use user_service::inbound::grpc::proto::user_service_server::UserServiceServer;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "user_service=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!(
        service = "user-service",
        version = env!("CARGO_PKG_VERSION"),
        "Service starting"
    );

    let config = Config::load()?;

    tracing::info!(
        database_url = %redact_credentials(&config.database.url),
        http_port = config.server.http_port,
        grpc_port = config.server.grpc_port,
        kafka_brokers = %config.kafka.brokers,
        kafka_topic = %config.kafka.topic,
        "Configuration loaded"
    );

    let pg_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database.url)
        .await?;
    tracing::info!(
        max_connections = 5,
        database = "postgresql",
        "Database connection pool created"
    );

    sqlx::migrate!("./migrations").run(&pg_pool).await?;
    tracing::info!(database = "postgresql", "Database migrations completed");

    let authenticator = Arc::new(Authenticator::new(config.jwt.secret.as_bytes()));
    let user_repository = Arc::new(UserRepository::new(pg_pool));
    let event_producer = Arc::new(EventProducer::new(&config)?);
    let password_hasher = Arc::new(PasswordHasher::new());

    let user_service = Arc::new(UserService::new(
        user_repository,
        event_producer,
        password_hasher,
    ));

    let http_address = format!("0.0.0.0:{}", config.server.http_port);
    let http_listener = tokio::net::TcpListener::bind(&http_address).await?;
    tracing::info!(
        address = %http_address,
        port = config.server.http_port,
        protocol = "http",
        "Http server listening"
    );

    let http_application = create_router(
        Arc::clone(&user_service),
        Arc::clone(&authenticator),
        config.jwt.expiration_hours,
    );
    let http_server = tokio::spawn(async move {
        axum::serve(http_listener, http_application)
            .with_graceful_shutdown(shutdown_signal())
            .await
    });

    let grpc_address = format!("0.0.0.0:{}", config.server.grpc_port).parse()?;
    let grpc_service = UserGrpcService::new(Arc::clone(&user_service));
    tracing::info!(
        address = %grpc_address,
        port = config.server.grpc_port,
        protocol = "grpc",
        "gRpc server listening"
    );

    let grpc_server = tokio::spawn(async move {
        Server::builder()
            .add_service(UserServiceServer::new(grpc_service))
            .serve_with_shutdown(grpc_address, shutdown_signal())
            .await
    });

    match tokio::try_join!(http_server, grpc_server) {
        Ok((_, _)) => tracing::info!("Servers exited successfully"),
        Err(e) => tracing::error!(error = %e, "Server error"),
    };

    Ok(())
}

/// Resolves on SIGINT or SIGTERM, so both servers stop accepting new
/// connections and drain in-flight requests instead of the process being
/// killed mid-request. Each server task calls this independently; tokio's
/// signal listeners broadcast to every registered receiver of the same kind.
async fn shutdown_signal() {
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
fn redact_credentials(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let after_scheme = &url[scheme_end + 3..];
    match after_scheme.find('@') {
        Some(at_pos) => format!("{}://***@{}", &url[..scheme_end], &after_scheme[at_pos + 1..]),
        None => url.to_string(),
    }
}
