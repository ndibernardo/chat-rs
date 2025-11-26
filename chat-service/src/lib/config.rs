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
}

/// PostgreSQL database configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
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
    pub num_shards: u32,
    pub user_events: UserEventsConfig,
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
