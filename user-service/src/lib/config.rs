use std::env;

use config::Config as ConfigBuilder;
use config::ConfigError;
use config::Environment;
use config::File;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub jwt: JwtConfig,
    pub kafka: KafkaConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub http_port: u16,
    pub grpc_port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub expiration_hours: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct KafkaConfig {
    pub brokers: String,
    pub topic: String,
}

impl Config {
    /// Load configuration from files with environment variable overrides
    ///
    /// Priority (highest to lowest):
    /// 1. Environment variables (DATABASE__URL, SERVER__HTTP_PORT, etc.)
    /// 2. Environment-specific config file (config/{environment}.toml)
    /// 3. Default config file (config/default.toml)
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

        let config: Config = configuration.try_deserialize()?;

        Ok(config)
    }
}
