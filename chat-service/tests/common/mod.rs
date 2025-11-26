use std::sync::Arc;

use auth::Authenticator;
use auth::Claims;
use auth::JwtHandler;
use chat_service::config::CassandraConfig;
use chat_service::config::Config;
use chat_service::config::DatabaseConfig;
use chat_service::config::JwtConfig;
use chat_service::config::KafkaConfig;
use chat_service::config::ServerConfig;
use chat_service::config::UserEventsConfig;
use chat_service::config::UserServiceConfig;
use chat_service::domain::channel::service::ChannelService;
use chat_service::domain::message::service::MessageService;
use chat_service::inbound::http::router::create_router;
use chat_service::inbound::websocket::registry::ConnectionRegistry;
use chat_service::outbound::events::message_publisher::KafkaMessageEventPublisher;
use chat_service::outbound::events::producer::KafkaEventProducer;
use chat_service::outbound::grpc::user::GrpcUserServiceClient;
use chat_service::outbound::repositories::channel::PostgresChannelRepository;
use chat_service::outbound::repositories::message::CassandraMessageRepository;
use scylla::Session;
use scylla::SessionBuilder;
use sqlx::postgres::PgConnectOptions;
use sqlx::postgres::PgPoolOptions;
use sqlx::Connection;
use sqlx::Executor;
use sqlx::PgConnection;
use sqlx::PgPool;

/// Test application that spawns a real server
pub struct TestApp {
    pub address: String,
    pub port: u16,
    pub db: TestDb,
    pub api_client: reqwest::Client,
    pub jwt_handler: JwtHandler,
}

/// Test database helper for chat-service
pub struct TestDb {
    pub pg_pool: PgPool,
    pub cassandra_session: Arc<Session>,
    pub pg_db_name: String,
    pub cassandra_keyspace: String,
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

        // Create repositories
        let channel_repo = Arc::new(PostgresChannelRepository::new(db.pg_pool.clone()));

        // Get configuration from environment
        let cassandra_nodes = std::env::var("CASSANDRA_NODES")
            .unwrap_or_else(|_| "localhost:9043".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .collect::<Vec<String>>();

        let kafka_brokers =
            std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());

        let user_service_url = std::env::var("USER_SERVICE_GRPC_URL")
            .unwrap_or_else(|_| "http://localhost:50052".to_string());

        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            format!(
                "postgresql://postgres:postgres@localhost:5433/{}",
                db.pg_db_name
            )
        });

        let config = Config {
            database: DatabaseConfig { url: database_url },
            cassandra: CassandraConfig {
                nodes: cassandra_nodes.clone(),
                keyspace: db.cassandra_keyspace.clone(),
            },
            server: ServerConfig { http_port: port },
            user_service: UserServiceConfig {
                grpc_url: user_service_url.clone(),
            },
            jwt: JwtConfig {
                secret: "test-secret-key-for-jwt-signing-at-least-32-bytes".to_string(),
                expiration_hours: 24,
            },
            kafka: KafkaConfig {
                brokers: kafka_brokers,
                group_id: format!("test-group-{}", uuid::Uuid::new_v4()),
                num_shards: 16,
                user_events: UserEventsConfig {
                    topic: "user-events-test".to_string(),
                    group_id: format!("test-user-events-{}", uuid::Uuid::new_v4()),
                },
            },
        };

        // Create adapters
        let message_repo = Arc::new(
            CassandraMessageRepository::new(&config)
                .await
                .expect("Failed to create message repository"),
        );

        let user_client = Arc::new(
            GrpcUserServiceClient::new(&user_service_url)
                .await
                .expect("Failed to create gRPC user service client"),
        );

        let kafka_producer =
            Arc::new(KafkaEventProducer::new(&config).expect("Failed to create Kafka producer"));
        let event_publisher = Arc::new(KafkaMessageEventPublisher::new(kafka_producer));

        // Create services
        let channel_service = Arc::new(ChannelService::new(channel_repo.clone()));
        let message_service = Arc::new(MessageService::new(
            message_repo,
            channel_repo,
            user_client,
            event_publisher,
        ));

        // Create WebSocket registry
        let connection_registry = Arc::new(ConnectionRegistry::new());

        // Create authenticator
        let authenticator = Arc::new(Authenticator::new(
            b"test-secret-key-for-jwt-signing-at-least-32-bytes",
        ));

        // Create router
        let router = create_router(
            channel_service,
            message_service,
            connection_registry,
            authenticator,
        );

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

    /// Create a test JWT token for a user ID
    pub fn create_token_for_user(&self, user_id: uuid::Uuid, username: &str) -> String {
        let claims = Claims::for_user(user_id.to_string(), username.to_string(), 24);
        self.jwt_handler
            .encode(&claims)
            .expect("Failed to create test token")
    }

    /// Create a test JWT token for a new random user
    pub fn create_test_token(&self) -> (String, uuid::Uuid) {
        let user_id = uuid::Uuid::new_v4();
        let token = self.create_token_for_user(user_id, "testuser");
        (token, user_id)
    }

    /// Helper to make GET request with authentication
    pub fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.api_client.get(&format!("{}{}", self.address, path))
    }

    /// Helper to make POST request with authentication
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
}

impl TestDb {
    /// Create a new test database environment with unique names
    pub async fn new() -> Self {
        let uuid_suffix = uuid::Uuid::new_v4().to_string().replace('-', "_");
        let pg_db_name = format!("test_chat_{}", uuid_suffix);
        let cassandra_keyspace = format!("test_chat_{}", uuid_suffix);

        // Setup PostgreSQL
        let postgres_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5433/postgres".to_string()
        });

        let mut conn = PgConnection::connect(&postgres_url)
            .await
            .expect("Failed to connect to Postgres");

        // Create test database
        conn.execute(format!(r#"CREATE DATABASE "{}";"#, pg_db_name).as_str())
            .await
            .expect("Failed to create test database");

        // Connect to the new test database
        let options = postgres_url
            .parse::<PgConnectOptions>()
            .expect("Failed to parse DATABASE_URL")
            .database(&pg_db_name);

        let pg_pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("Failed to connect to test database");

        // Run migrations
        sqlx::migrate!("./migrations")
            .run(&pg_pool)
            .await
            .expect("Failed to run migrations");

        // Setup Cassandra
        let cassandra_nodes = std::env::var("CASSANDRA_NODES")
            .unwrap_or_else(|_| "localhost:9043".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .collect::<Vec<String>>();

        let cassandra_session = SessionBuilder::new()
            .known_nodes(&cassandra_nodes)
            .build()
            .await
            .expect("Failed to connect to Cassandra");

        // Create keyspace
        cassandra_session
            .query(
                format!(
                    "CREATE KEYSPACE IF NOT EXISTS {} WITH replication = {{'class': 'SimpleStrategy', 'replication_factor': 1}}",
                    cassandra_keyspace
                ),
                &[],
            )
            .await
            .expect("Failed to create Cassandra keyspace");

        // Use keyspace
        cassandra_session
            .use_keyspace(&cassandra_keyspace, false)
            .await
            .expect("Failed to use Cassandra keyspace");

        // Create messages_by_channel table
        cassandra_session
            .query(
                "CREATE TABLE IF NOT EXISTS messages_by_channel (
                    channel_id uuid,
                    message_id timeuuid,
                    user_id uuid,
                    content text,
                    timestamp timestamp,
                    PRIMARY KEY (channel_id, message_id)
                ) WITH CLUSTERING ORDER BY (message_id DESC)",
                &[],
            )
            .await
            .expect("Failed to create messages_by_channel table");

        // Create messages_by_user table
        cassandra_session
            .query(
                "CREATE TABLE IF NOT EXISTS messages_by_user (
                    user_id uuid,
                    message_id timeuuid,
                    channel_id uuid,
                    content text,
                    timestamp timestamp,
                    PRIMARY KEY (user_id, message_id)
                ) WITH CLUSTERING ORDER BY (message_id DESC)",
                &[],
            )
            .await
            .expect("Failed to create messages_by_user table");

        Self {
            pg_pool,
            cassandra_session: Arc::new(cassandra_session),
            pg_db_name,
            cassandra_keyspace,
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // Cleanup databases asynchronously
        let pg_db_name = self.pg_db_name.clone();
        let cassandra_keyspace = self.cassandra_keyspace.clone();
        let cassandra_session = self.cassandra_session.clone();

        tokio::spawn(async move {
            // Cleanup Cassandra keyspace
            let _ = cassandra_session
                .query(
                    format!("DROP KEYSPACE IF EXISTS {}", cassandra_keyspace),
                    &[],
                )
                .await;

            // Cleanup PostgreSQL database
            let postgres_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgresql://postgres:postgres@localhost:5433/postgres".to_string()
            });

            if let Ok(mut conn) = PgConnection::connect(&postgres_url).await {
                // Terminate existing connections
                let _ = conn
                    .execute(
                        format!(
                            r#"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}';"#,
                            pg_db_name
                        )
                        .as_str(),
                    )
                    .await;

                // Drop database
                let _ = conn
                    .execute(format!(r#"DROP DATABASE IF EXISTS "{}";"#, pg_db_name).as_str())
                    .await;
            }
        });
    }
}
