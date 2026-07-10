mod common;

use std::sync::Arc;
use std::time::Duration;

use chat_service::config::CassandraConfig;
use chat_service::config::Config;
use chat_service::config::CorsConfig;
use chat_service::config::DatabaseConfig;
use chat_service::config::DlqConfig;
use chat_service::config::JwtConfig;
use chat_service::config::KafkaConfig;
use chat_service::config::OutboxConfig;
use chat_service::config::PersisterConfig;
use chat_service::config::ServerConfig;
use chat_service::config::ShutdownConfig;
use chat_service::config::UserEventsConfig;
use chat_service::config::UserServiceConfig;
use chat_service::config::WebsocketConfig;
use chat_service::domain::channel::models::ChannelId;
use chat_service::domain::message::models::Limit;
use chat_service::domain::message::models::MessageId;
use chat_service::domain::message::ports::MessageRepository as _;
use chat_service::domain::user::models::UserId;
use chat_service::inbound::kafka::MessagePersister;
use chat_service::outbound::kafka::envelope::SCHEMA_CHAT_V1;
use chat_service::outbound::kafka::messages::ChatEventMessage;
use chat_service::outbound::kafka::messages::MessageSentMessage;
use chat_service::outbound::kafka::producer::EventProducer;
use chat_service::outbound::scylla::MessageRepository as ScyllaMessageRepository;
use common::TestDb;
use tokio_util::sync::CancellationToken;

fn persister_test_config(test_db: &TestDb, messages_topic: String) -> Config {
    let kafka_brokers =
        std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());
    let cassandra_nodes = std::env::var("CASSANDRA_NODES")
        .unwrap_or_else(|_| "localhost:9042".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .collect::<Vec<String>>();

    Config {
        database: DatabaseConfig {
            url: "postgresql://unused".to_string(),
            max_connections: 5,
        },
        cassandra: CassandraConfig {
            nodes: cassandra_nodes,
            keyspace: test_db.cassandra_keyspace.clone(),
            replication_strategy: "SimpleStrategy".to_string(),
            replication_factor: 1,
            datacenter: None,
        },
        server: ServerConfig {
            http_port: 0,
            metrics_port: 0,
        },
        user_service: UserServiceConfig {
            grpc_url: "http://unused".to_string(),
        },
        jwt: JwtConfig {
            public_key_path: "../../keys/dev/jwt_ed25519.pub.pem".to_string(),
        },
        cors: CorsConfig {
            allowed_origins: vec!["http://localhost:5173".to_string()],
        },
        kafka: KafkaConfig {
            brokers: kafka_brokers,
            group_id: format!("test-group-{}", uuid::Uuid::new_v4()),
            messages_topic,
            delivery_timeout_ms: 10_000,
            instance_id: Some(format!("test-instance-{}", uuid::Uuid::new_v4())),
            user_events: UserEventsConfig {
                topic: "user-events-test".to_string(),
                group_id: format!("test-user-events-{}", uuid::Uuid::new_v4()),
            },
            dlq: DlqConfig::default(),
            persister: PersisterConfig {
                group_id: format!("test-persister-{}", uuid::Uuid::new_v4()),
            },
        },
        websocket: WebsocketConfig::default(),
        shutdown: ShutdownConfig::default(),
        outbox: OutboxConfig::default(),
    }
}

/// Proves the Kafka-first message path end to end: a `MessageSent` envelope
/// published straight to the chat topic (as `send_message` does, without
/// ever touching Cassandra itself) is picked up by `MessagePersister` and
/// lands in Cassandra history — with no direct Scylla write on the send path.
#[tokio::test]
async fn persister_writes_a_kafka_first_message_sent_event_into_cassandra() {
    // Arrange
    let test_db = TestDb::new().await;
    let messages_topic = format!("test-chat-messages-{}", uuid::Uuid::new_v4());
    let config = persister_test_config(&test_db, messages_topic.clone());

    let message_repository = Arc::new(
        ScyllaMessageRepository::new(&config)
            .await
            .expect("Failed to create Scylla message repository"),
    );

    let persister = MessagePersister::new(&config, Arc::clone(&message_repository))
        .expect("Failed to create message persister");
    let cancellation = CancellationToken::new();
    let persister_token = cancellation.clone();
    let persister_handle = tokio::spawn(async move {
        persister.start_consuming(persister_token).await;
    });

    let producer = EventProducer::new(&config).expect("Failed to create Kafka producer");

    let channel_id = ChannelId::new();
    let user_id = UserId::new();
    let message_id = MessageId::new_time_based();

    let event = ChatEventMessage::MessageSent(MessageSentMessage {
        event_id: uuid::Uuid::new_v4().to_string(),
        message_id: message_id.to_string(),
        channel_id: channel_id.to_string(),
        user_id: user_id.to_string(),
        content: "Deploy to production is green".to_string(),
        timestamp: chrono::Utc::now(),
    });

    // Act
    producer
        .publish_event(channel_id, SCHEMA_CHAT_V1, event)
        .await
        .expect("Failed to publish MessageSent event");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let limit = Limit::new(10).unwrap();
    let persisted = loop {
        let messages = message_repository
            .find_by_channel(channel_id, limit, None)
            .await
            .expect("Failed to query Cassandra for persisted messages");

        if !messages.is_empty() {
            break messages;
        }

        assert!(
            tokio::time::Instant::now() < deadline,
            "Persister did not write the message to Cassandra within the deadline"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    // Assert
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].channel_id(), channel_id);
    assert_eq!(persisted[0].user_id(), user_id);
    assert_eq!(
        persisted[0].content().as_str(),
        "Deploy to production is green"
    );

    cancellation.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), persister_handle).await;
}
