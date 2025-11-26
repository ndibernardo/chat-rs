use std::sync::Arc;

use auth::Authenticator;
use auth::JwtHandler;
use sqlx::postgres::PgConnectOptions;
use sqlx::postgres::PgPoolOptions;
use sqlx::Connection;
use sqlx::Executor;
use sqlx::PgConnection;
use sqlx::PgPool;
use user_service::config::Config;
use user_service::config::DatabaseConfig;
use user_service::config::JwtConfig;
use user_service::config::KafkaConfig;
use user_service::config::ServerConfig;
use user_service::domain::user::service::UserService;
use user_service::inbound::http::router::create_router;
use user_service::outbound::events::KafkaEventProducer;
use user_service::outbound::repositories::user::PostgresUserRepository;

/// Test application that spawns a real server
pub struct TestApp {
    pub address: String,
    pub port: u16,
    pub db: TestDb,
    pub api_client: reqwest::Client,
    pub jwt_handler: JwtHandler,
}

/// Test database helper
pub struct TestDb {
    pub pool: PgPool,
    pub db_name: String,
}

impl TestApp {
    /// Spawn the application in a background task and return TestApp
    pub async fn spawn() -> Self {
        let db = TestDb::new().await;

        // Use random port (0 = OS assigns)
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind random port");
        let port = listener.local_addr().unwrap().port();
        let address = format!("http://127.0.0.1:{}", port);

        // Create repository
        let user_repo = Arc::new(PostgresUserRepository::new(db.pool.clone()));

        // Get configuration from environment
        let kafka_brokers =
            std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());
        let kafka_topic =
            std::env::var("KAFKA__TOPIC").unwrap_or_else(|_| "user-events-test".to_string());

        let config = Config {
            database: DatabaseConfig {
                url: format!(
                    "postgresql://postgres:postgres@localhost:5433/{}",
                    db.db_name
                ),
            },
            server: ServerConfig {
                http_port: port,
                grpc_port: 50051,
            },
            jwt: JwtConfig {
                secret: "test-secret-key-for-jwt-signing-at-least-32-bytes".to_string(),
                expiration_hours: 24,
            },
            kafka: KafkaConfig {
                brokers: kafka_brokers,
                topic: kafka_topic,
            },
        };

        let event_publisher = Arc::new(
            KafkaEventProducer::new(&config)
                .expect("Failed to create Kafka event producer for tests"),
        );

        let user_service = Arc::new(UserService::new(user_repo, event_publisher));

        // Create authenticator
        let authenticator = Arc::new(Authenticator::new(
            b"test-secret-key-for-jwt-signing-at-least-32-bytes",
        ));

        let router = create_router(user_service, authenticator, 24);

        // Spawn server in background
        tokio::spawn(async move {
            axum::serve(listener, router).await.expect("Server error");
        });

        let jwt_handler = JwtHandler::new(b"test-secret-key-for-jwt-signing-at-least-32-bytes");

        Self {
            address,
            port,
            db,
            api_client: reqwest::Client::builder()
                .cookie_store(true)
                .build()
                .expect("Failed to create reqwest client"),
            jwt_handler,
        }
    }

    /// Helper to make GET request
    pub fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.api_client.get(&format!("{}{}", self.address, path))
    }

    /// Helper to make POST request
    pub fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.api_client.post(&format!("{}{}", self.address, path))
    }

    /// Helper to make GET request with Bearer token
    pub fn get_authenticated(&self, path: &str, token: &str) -> reqwest::RequestBuilder {
        self.get(path).bearer_auth(token)
    }

    /// Helper to make POST request with Bearer token
    pub fn post_authenticated(&self, path: &str, token: &str) -> reqwest::RequestBuilder {
        self.post(path).bearer_auth(token)
    }

    /// Helper to make PATCH request with Bearer token
    pub fn patch_authenticated(&self, path: &str, token: &str) -> reqwest::RequestBuilder {
        self.api_client
            .patch(&format!("{}{}", self.address, path))
            .bearer_auth(token)
    }

    /// Helper to make DELETE request with Bearer token
    pub fn delete_authenticated(&self, path: &str, token: &str) -> reqwest::RequestBuilder {
        self.api_client
            .delete(&format!("{}{}", self.address, path))
            .bearer_auth(token)
    }
}

impl TestDb {
    /// Create a new test database with a unique name
    pub async fn new() -> Self {
        let db_name = format!(
            "test_user_service_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "_")
        );

        // Connect to postgres database to create test database (defaults to test port 5433)
        let postgres_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5433/postgres".to_string()
        });

        let mut conn = PgConnection::connect(&postgres_url)
            .await
            .expect("Failed to connect to Postgres");

        // Create test database
        conn.execute(format!(r#"CREATE DATABASE "{}";"#, db_name).as_str())
            .await
            .expect("Failed to create test database");

        // Connect to the new test database
        let options = postgres_url
            .parse::<PgConnectOptions>()
            .expect("Failed to parse DATABASE_URL")
            .database(&db_name);

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("Failed to connect to test database");

        // Run migrations
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("Failed to run migrations");

        Self { pool, db_name }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // Database cleanup happens asynchronously
        let db_name = self.db_name.clone();
        tokio::spawn(async move {
            let postgres_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgresql://postgres:postgres@localhost:5433/postgres".to_string()
            });

            if let Ok(mut conn) = PgConnection::connect(&postgres_url).await {
                // Terminate existing connections
                let _ = conn.execute(
                    format!(
                        r#"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}';"#,
                        db_name
                    ).as_str()
                ).await;

                // Drop database
                let _ = conn
                    .execute(format!(r#"DROP DATABASE IF EXISTS "{}";"#, db_name).as_str())
                    .await;
            }
        });
    }
}
