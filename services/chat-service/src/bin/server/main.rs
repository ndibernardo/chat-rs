use chat_service::config::Config;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // JSON logs for log aggregators in deployed environments; human-readable
    // by default for local development.
    let fmt_layer = if std::env::var("LOG_FORMAT").as_deref() == Ok("json") {
        tracing_subscriber::fmt::layer().json().boxed()
    } else {
        tracing_subscriber::fmt::layer().boxed()
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "chat_service=debug,tower_http=debug".into()),
        )
        .with(fmt_layer)
        .init();

    tracing::info!(
        service = "chat-service",
        version = env!("CARGO_PKG_VERSION"),
        "Service starting"
    );

    let config = Config::load()?;

    chat_service::bootstrap::run(config).await
}
