use std::env;

use config::Config as ConfigBuilder;
use config::ConfigError;
use config::Environment;
use config::File;
use serde::Deserialize;

/// Application configuration for chat-service.
///
/// Loaded from configuration files with environment variable overrides.
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub database: DatabaseConfig,
    pub cassandra: CassandraConfig,
    pub server: ServerConfig,
    pub user_service: UserServiceConfig,
    pub kafka: KafkaConfig,
    pub jwt: JwtConfig,
    #[serde(default)]
    pub websocket: WebsocketConfig,
}

/// PostgreSQL database configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

fn default_max_connections() -> u32 {
    5
}

/// WebSocket gateway configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct WebsocketConfig {
    /// Bound on each connection's outbound send queue. A connection that
    /// can't keep up gets disconnected rather than let the queue grow
    /// without limit — unbounded per-connection queues are a memory-DoS
    /// vector under a slow or stalled client.
    #[serde(default = "default_send_queue_capacity")]
    pub send_queue_capacity: usize,
}

impl Default for WebsocketConfig {
    fn default() -> Self {
        Self {
            send_queue_capacity: default_send_queue_capacity(),
        }
    }
}

fn default_send_queue_capacity() -> usize {
    256
}

/// Cassandra database configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct CassandraConfig {
    pub nodes: Vec<String>,
    pub keyspace: String,
}

/// HTTP server configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub http_port: u16,
}

/// User-service gRPC client configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct UserServiceConfig {
    pub grpc_url: String,
}

/// Kafka event broker configuration.
///
/// Used for publishing message events and consuming user events.
#[derive(Debug, Deserialize, Clone)]
pub struct KafkaConfig {
    pub brokers: String,
    pub group_id: String,
    /// The single topic all chat messages are produced to and consumed
    /// from, keyed by `channel_id` for per-channel ordering.
    #[serde(default = "default_messages_topic")]
    pub messages_topic: String,
    /// Upper bound (ms) on how long the producer buffers and retries a
    /// message before giving up (`message.timeout.ms`).
    #[serde(default = "default_delivery_timeout_ms")]
    pub delivery_timeout_ms: u64,
    /// Stable per-process identity used for Kafka static group membership
    /// (`group.instance.id`), so a restarted pod rejoins its group instead
    /// of triggering a rebalance. Explicit override; absent in most
    /// deployments, where it is instead resolved from `POD_NAME`/hostname.
    #[serde(default)]
    pub instance_id: Option<String>,
    pub user_events: UserEventsConfig,
}

fn default_messages_topic() -> String {
    "chat.messages".to_string()
}

fn default_delivery_timeout_ms() -> u64 {
    10_000
}

/// User events Kafka consumer configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct UserEventsConfig {
    pub topic: String,
    pub group_id: String,
}

/// JWT authentication configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub expiration_hours: i64,
}

impl Config {
    /// Load configuration from files with environment variable overrides.
    ///
    /// # Configuration Priority (highest to lowest)
    /// 1. Environment variables (DATABASE__URL, SERVER__HTTP_PORT, etc.)
    /// 2. Environment-specific config file (config/{environment}.toml)
    /// 3. Default config file (config/default.toml)
    ///
    /// # Returns
    /// Loaded configuration
    ///
    /// # Errors
    /// Returns error if required configuration values are missing or invalid
    pub fn load() -> Result<Self, ConfigError> {
        let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "development".to_string());

        let configuration = ConfigBuilder::builder()
            // Start with default configuration
            .add_source(File::with_name("config/default").required(false))
            // Layer on environment-specific configuration
            .add_source(File::with_name(&format!("config/{}", run_mode)).required(false))
            // Layer on environment variables (with __ as separator)
            // Example: DATABASE__URL=postgres://... overrides database.url
            .add_source(Environment::with_prefix("").separator("__"))
            .build()?;

        configuration.try_deserialize()
    }

    /// Legacy method for backward compatibility.
    ///
    /// # Returns
    /// Loaded configuration
    ///
    /// # Errors
    /// Returns error if required configuration values are missing or invalid
    ///
    /// # Deprecated
    /// Use `Config::load()` instead
    #[deprecated(note = "Use Config::load() instead")]
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::load()?)
    }
}
