mod common;

use std::time::Duration;

use chat_service::config::CassandraConfig;
use chat_service::config::Config;
use chat_service::config::CorsConfig;
use chat_service::config::DatabaseConfig;
use chat_service::config::JwtConfig;
use chat_service::config::KafkaConfig;
use chat_service::config::ServerConfig;
use chat_service::config::ShutdownConfig;
use chat_service::config::UserEventsConfig;
use chat_service::config::UserServiceConfig;
use chat_service::config::WebsocketConfig;
use chat_service::domain::channel::events::ChannelCreatedEvent;
use chat_service::domain::channel::models::Channel;
use chat_service::domain::channel::models::ChannelId;
use chat_service::domain::channel::models::ChannelName;
use chat_service::domain::message::events::MessageSentEvent;
use chat_service::domain::message::models::Message;
use chat_service::domain::message::models::MessageContent;
use chat_service::domain::user::models::UserId;
use chat_service::outbound::kafka::messages::ChannelCreatedMessage;
use chat_service::outbound::kafka::messages::ChatEventMessage;
use chat_service::outbound::kafka::messages::MessageSentMessage;
use chat_service::outbound::kafka::producer::EventProducer;
use common::TestDb;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::Consumer;
use rdkafka::consumer::StreamConsumer;
use rdkafka::message::Message as KafkaMessage;
use tokio::time::timeout;

const TEST_MESSAGES_TOPIC: &str = "chat.messages";

/// Helper to create Kafka producer for testing
fn create_kafka_producer(kafka_brokers: &str) -> EventProducer {
    let config = Config {
        database: DatabaseConfig {
            url: "postgresql://unused".to_string(),
            max_connections: 5,
        },
        cassandra: CassandraConfig {
            nodes: vec!["unused".to_string()],
            keyspace: "unused".to_string(),
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
            brokers: kafka_brokers.to_string(),
            group_id: format!("test-group-{}", uuid::Uuid::new_v4()),
            messages_topic: TEST_MESSAGES_TOPIC.to_string(),
            delivery_timeout_ms: 10_000,
            instance_id: Some(format!("test-instance-{}", uuid::Uuid::new_v4())),
            user_events: UserEventsConfig {
                topic: "user-events-test".to_string(),
                group_id: format!("test-user-events-{}", uuid::Uuid::new_v4()),
            },
        },
        websocket: WebsocketConfig::default(),
        shutdown: ShutdownConfig::default(),
    };

    EventProducer::new(&config).expect("Failed to create Kafka producer")
}

/// Test that Kafka producer can publish message events
#[tokio::test]
async fn test_kafka_publish_message_event() {
    let kafka_brokers =
        std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());

    let _test_db = TestDb::new().await;
    let kafka_producer = create_kafka_producer(&kafka_brokers);

    // Create a test message and event
    let channel_id = ChannelId::new();
    let message = Message::new(
        channel_id,
        UserId::new(),
        MessageContent::new("Test message content".to_string()).unwrap(),
    );

    let event = MessageSentEvent::new(&message);

    // Wrap in serializable message envelope
    let message_envelope = MessageSentMessage::from(&event);
    let envelope = ChatEventMessage::MessageSent(message_envelope);

    let result = kafka_producer.publish_event(channel_id, &envelope).await;

    assert!(
        result.is_ok(),
        "Failed to publish event: {:?}",
        result.err()
    );
}

/// Test that Kafka producer can publish channel events
#[tokio::test]
async fn test_kafka_publish_channel_event() {
    let kafka_brokers =
        std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());

    let _test_db = TestDb::new().await;
    let kafka_producer = create_kafka_producer(&kafka_brokers);

    // Create a test channel and event
    let channel = Channel::new_public(
        ChannelName::new("test-channel").unwrap(),
        Some("Test channel".to_string()),
        UserId::new(),
    );
    let channel_id = channel.id();

    let event = ChannelCreatedEvent::new(&channel);

    // Wrap in serializable message envelope
    let message_envelope = ChannelCreatedMessage::from(&event);
    let envelope = ChatEventMessage::ChannelCreated(message_envelope);

    let result = kafka_producer.publish_event(channel_id, &envelope).await;

    assert!(
        result.is_ok(),
        "Failed to publish event: {:?}",
        result.err()
    );
}

/// Test that published events can be consumed from the messages topic
#[tokio::test]
async fn test_kafka_publish_and_consume() {
    let kafka_brokers =
        std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());

    let _test_db = TestDb::new().await;
    let kafka_producer = create_kafka_producer(&kafka_brokers);

    // Create and publish a test message and event
    let channel_id = ChannelId::new();
    let message = Message::new(
        channel_id,
        UserId::new(),
        MessageContent::new("Test consume message".to_string()).unwrap(),
    );

    let event = MessageSentEvent::new(&message);
    let message_id = event.message_id;

    // Wrap in serializable message envelope
    let message_envelope = MessageSentMessage::from(&event);
    let envelope = ChatEventMessage::MessageSent(message_envelope);

    kafka_producer
        .publish_event(channel_id, &envelope)
        .await
        .expect("Failed to publish event");

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &kafka_brokers)
        .set(
            "group.id",
            format!("test-consumer-group-{}", uuid::Uuid::new_v4()),
        )
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "true")
        .create()
        .expect("Failed to create consumer");

    consumer
        .subscribe(&[TEST_MESSAGES_TOPIC])
        .expect("Failed to subscribe to topic");

    // Try to consume the message with timeout
    let consume_result = timeout(Duration::from_secs(10), async {
        use futures::StreamExt;

        let mut stream = consumer.stream();
        while let Some(message_result) = stream.next().await {
            match message_result {
                Ok(msg) => {
                    let payload = msg.payload().expect("Message has no payload");
                    let payload_str = std::str::from_utf8(payload).expect("Invalid UTF-8");

                    // Try to deserialize as ChatEventMessage
                    if let Ok(received_envelope) =
                        serde_json::from_str::<ChatEventMessage>(payload_str)
                        && let ChatEventMessage::MessageSent(received_msg) = received_envelope
                        && received_msg.message_id == message_id.to_string()
                    {
                        return Some(received_msg);
                    }
                }
                Err(e) => {
                    eprintln!("Error consuming message: {:?}", e);
                }
            }
        }
        None
    })
    .await;

    assert!(consume_result.is_ok(), "Timed out waiting for message");

    let received = consume_result.unwrap();
    assert!(received.is_some(), "Did not receive the published event");

    let received_msg = received.unwrap();
    assert_eq!(received_msg.message_id, message_id.to_string());
    assert_eq!(received_msg.content, "Test consume message");
}

/// Test publishing multiple events for the same channel
#[tokio::test]
async fn test_kafka_publish_multiple_events() {
    let kafka_brokers =
        std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());

    let _test_db = TestDb::new().await;
    let kafka_producer = create_kafka_producer(&kafka_brokers);

    let channel_id = ChannelId::new();

    // Publish multiple events
    for i in 0..5 {
        let message = Message::new(
            channel_id,
            UserId::new(),
            MessageContent::new(format!("Test message {}", i)).unwrap(),
        );

        let event = MessageSentEvent::new(&message);

        // Wrap in serializable message envelope
        let message_envelope = MessageSentMessage::from(&event);
        let envelope = ChatEventMessage::MessageSent(message_envelope);

        let result = kafka_producer.publish_event(channel_id, &envelope).await;

        assert!(
            result.is_ok(),
            "Failed to publish event {}: {:?}",
            i,
            result.err()
        );
    }
}

/// Test error handling when publishing to invalid broker
#[tokio::test]
async fn test_kafka_error_handling() {
    // Use invalid brokers to force an error
    let _test_db = TestDb::new().await;

    // This should succeed in creating the producer but fail when publishing
    let kafka_brokers = "invalid-broker:9999";
    let kafka_producer = create_kafka_producer(kafka_brokers);

    let channel_id = ChannelId::new();
    let message = Message::new(
        channel_id,
        UserId::new(),
        MessageContent::new("Test message".to_string()).unwrap(),
    );

    let event = MessageSentEvent::new(&message);

    // Wrap in serializable message envelope
    let message_envelope = MessageSentMessage::from(&event);
    let envelope = ChatEventMessage::MessageSent(message_envelope);

    // This should fail with timeout or connection error
    let result = timeout(
        Duration::from_secs(7),
        kafka_producer.publish_event(channel_id, &envelope),
    )
    .await;

    // Either timeout or error from Kafka
    assert!(
        result.is_err() || result.unwrap().is_err(),
        "Expected error when publishing to invalid broker"
    );
}
