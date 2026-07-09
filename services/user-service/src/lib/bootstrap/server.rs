use std::sync::Arc;
use std::time::Duration;

use auth::Authenticator;
use tonic::transport::Server;
use web::health::HealthState;
use web::health::PgReadyCheck;
use web::health::PgSchemaReadyCheck;
use web::health::ReadyCheck;
use web::health::health_router;

use super::common;
use crate::config::Config;
use crate::domain::user::service::Service as UserService;
use crate::inbound::grpc::UserGrpcService;
use crate::inbound::grpc::proto::user_service_server::UserServiceServer;
use crate::inbound::http::router::create_router;
use crate::outbound::argon2::PasswordHasher;
use crate::outbound::kafka::EventProducer;
use crate::outbound::postgres::UserRepository;

/// `user-service --role server`: HTTP + gRPC API. Today's single-binary
/// behavior and the local-dev default.
pub async fn run(config: Config) -> Result<(), anyhow::Error> {
    tracing::info!(
        database_url = %common::redact_credentials(&config.database.url),
        http_port = config.server.http_port,
        grpc_port = config.server.grpc_port,
        kafka_brokers = %config.kafka.brokers,
        kafka_topic = %config.kafka.topic,
        "Configuration loaded"
    );
    web::metrics::install_prometheus_recorder(config.server.metrics_port)?;

    let pg_pool = common::connect_pg_pool(&config).await?;

    let private_key_pem = std::fs::read(&config.jwt.private_key_path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to read JWT private key at {}: {e}",
            config.jwt.private_key_path
        )
    })?;
    let public_key_pem = std::fs::read(&config.jwt.public_key_path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to read JWT public key at {}: {e}",
            config.jwt.public_key_path
        )
    })?;
    let authenticator = Arc::new(
        Authenticator::signer(&private_key_pem, &public_key_pem)
            .map_err(|e| anyhow::anyhow!("Failed to build JWT signer: {e}"))?,
    );
    let user_repository = Arc::new(UserRepository::new(pg_pool.clone()));
    let event_producer = Arc::new(EventProducer::new(&config)?);
    let password_hasher = Arc::new(PasswordHasher::new());

    let user_service = Arc::new(UserService::new(
        user_repository,
        event_producer,
        password_hasher,
    ));

    let checks: Vec<Arc<dyn ReadyCheck>> = vec![
        Arc::new(PgReadyCheck::new(pg_pool.clone())),
        Arc::new(PgSchemaReadyCheck::new(
            pg_pool,
            sqlx::migrate!("./migrations"),
        )),
    ];

    let http_address = format!("0.0.0.0:{}", config.server.http_port);
    let http_listener = tokio::net::TcpListener::bind(&http_address).await?;
    tracing::info!(
        address = %http_address,
        port = config.server.http_port,
        protocol = "http",
        "Http server listening"
    );

    let health_state = HealthState::new(checks);
    let draining = health_state.draining_flag();
    let readiness_delay = Duration::from_secs(config.shutdown.readiness_delay_seconds);

    let http_application = create_router(
        Arc::clone(&user_service),
        Arc::clone(&authenticator),
        config.jwt.expiration_hours,
        &config.cors.allowed_origins,
    )?
    .merge(health_router(health_state));
    let http_shutdown_draining = Arc::clone(&draining);
    let http_server = tokio::spawn(async move {
        axum::serve(http_listener, http_application)
            .with_graceful_shutdown(common::graceful_shutdown(
                http_shutdown_draining,
                readiness_delay,
            ))
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
            .serve_with_shutdown(
                grpc_address,
                common::graceful_shutdown(draining, readiness_delay),
            )
            .await
    });

    match tokio::try_join!(http_server, grpc_server) {
        Ok((_, _)) => tracing::info!("Servers exited successfully"),
        Err(e) => tracing::error!(error = %e, "Server error"),
    };

    Ok(())
}
