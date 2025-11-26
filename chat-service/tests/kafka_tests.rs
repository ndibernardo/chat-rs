mod common;

use std::time::Duration;

use chat_service::config::CassandraConfig;
use chat_service::config::Config;
use chat_service::config::DatabaseConfig;
use chat_service::config::JwtConfig;
use chat_service::config::KafkaConfig;
use chat_service::config::ServerConfig;
use chat_service::config::UserEventsConfig;
use chat_service::config::UserServiceConfig;
use chat_service::domain::channel::events::ChannelCreatedEvent;
use chat_service::domain::channel::models::Channel;
use chat_service::domain::channel::models::ChannelId;
use chat_service::domain::channel::models::ChannelName;
use chat_service::domain::channel::models::PublicChannel;
use chat_service::domain::message::events::MessageSentEvent;
use chat_service::domain::message::models::Message;
use chat_service::domain::message::models::MessageContent;
use chat_service::domain::message::models::MessageId;
use chat_service::domain::user::models::UserId;
use chat_service::outbound::events::messages::ChannelCreatedMessage;
use chat_service::outbound::events::messages::ChatEventMessage;
use chat_service::outbound::events::messages::MessageSentMessage;
use chat_service::outbound::events::producer::KafkaEventProducer;
use common::TestDb;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::Consumer;
use rdkafka::consumer::StreamConsumer;
use rdkafka::message::Message as KafkaMessage;
use tokio::time::timeout;

/// Helper to create Kafka producer for testing
fn create_kafka_producer(kafka_brokers: &str) -> KafkaEventProducer {
    let config = Config {
        database: DatabaseConfig {
            url: "postgresql://unused".to_string(),
        },
        cassandra: CassandraConfig {
            nodes: vec!["unused".to_string()],
            keyspace: "unused".to_string(),
        },
        server: ServerConfig { http_port: 0 },
        user_service: UserServiceConfig {
            grpc_url: "http://unused".to_string(),
        },
        jwt: JwtConfig {
            secret: "unused".to_string(),
            expiration_hours: 24,
        },
        kafka: KafkaConfig {
            brokers: kafka_brokers.to_string(),
            group_id: format!("test-group-{}", uuid::Uuid::new_v4()),
            num_shards: 16,
            user_events: UserEventsConfig {
                topic: "user-events-test".to_string(),
                group_id: format!("test-user-events-{}", uuid::Uuid::new_v4()),
            },
        },
    };

    KafkaEventProducer::new(&config).expect("Failed to create Kafka producer")
}

/// Test that Kafka producer can publish events to sharded topics
#[tokio::test]
async fn test_kafka_publish_message_event() {
    let kafka_brokers =
        std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());

    let _test_db = TestDb::new().await;
    let kafka_producer = create_kafka_producer(&kafka_brokers);

    // Create a test message and event
    let channel_id = ChannelId::new();
    let message = Message {
        id: MessageId::new_time_based(),
        channel_id,
        user_id: UserId(uuid::Uuid::new_v4()),
        content: MessageContent::new("Test message content".to_string()).unwrap(),
        timestamp: chrono::Utc::now(),
    };

    let event = MessageSentEvent::new(&message);
    let key = event.message_id.to_string();

    // Wrap in serializable message envelope
    let message_envelope = MessageSentMessage::from(&event);
    let envelope = ChatEventMessage::MessageSent(message_envelope);

    // Publish the event (will be sharded based on channel_id)
    let result = kafka_producer
        .publish_event(channel_id, &key, &envelope)
        .await;

    assert!(
        result.is_ok(),
        "Failed to publish event: {:?}",
        result.err()
    );
}

/// Test that Kafka producer can publish channel events to sharded topics
#[tokio::test]
async fn test_kafka_publish_channel_event() {
    let kafka_brokers =
        std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());

    let _test_db = TestDb::new().await;
    let kafka_producer = create_kafka_producer(&kafka_brokers);

    // Create a test channel and event
    let channel_id = ChannelId::new();
    let public_channel = PublicChannel {
        id: channel_id,
        name: ChannelName::new("test-channel".to_string()).unwrap(),
        description: Some("Test channel".to_string()),
        created_by: UserId(uuid::Uuid::new_v4()),
        created_at: chrono::Utc::now(),
    };
    let channel = Channel::Public(public_channel);

    let event = ChannelCreatedEvent::new(&channel);
    let key = event.channel_id.to_string();

    // Wrap in serializable message envelope
    let message_envelope = ChannelCreatedMessage::from(&event);
    let envelope = ChatEventMessage::ChannelCreated(message_envelope);

    // Publish the event (will be sharded based on channel_id)
    let result = kafka_producer
        .publish_event(channel_id, &key, &envelope)
        .await;

    assert!(
        result.is_ok(),
        "Failed to publish event: {:?}",
        result.err()
    );
}

/// Test that published events can be consumed from sharded topics
#[tokio::test]
async fn test_kafka_publish_and_consume() {
    let kafka_brokers =
        std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());

    let _test_db = TestDb::new().await;
    let kafka_producer = create_kafka_producer(&kafka_brokers);

    // Create and publish a test message and event
    let channel_id = ChannelId::new();
    let message = Message {
        id: MessageId::new_time_based(),
        channel_id,
        user_id: UserId(uuid::Uuid::new_v4()),
        content: MessageContent::new("Test consume message".to_string()).unwrap(),
        timestamp: chrono::Utc::now(),
    };

    let event = MessageSentEvent::new(&message);
    let key = event.message_id.to_string();
    let message_id = event.message_id;

    // Wrap in serializable message envelope
    let message_envelope = MessageSentMessage::from(&event);
    let envelope = ChatEventMessage::MessageSent(message_envelope);

    // Publish the event (will go to a sharded topic based on channel_id)
    kafka_producer
        .publish_event(channel_id, &key, &envelope)
        .await
        .expect("Failed to publish event");

    // Calculate which shard this channel_id maps to
    use chat_service::outbound::events::topic::TopicSharder;
    let sharder = TopicSharder::new(16, "chat.messages").unwrap();
    let topic = sharder.get_shard_for_channel(channel_id);

    // Create a consumer for the specific shard
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &kafka_brokers)
        .set("group.id", "test-consumer-group")
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "true")
        .create()
        .expect("Failed to create consumer");

    consumer
        .subscribe(&[&topic])
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
                    {
                        if let ChatEventMessage::MessageSent(received_msg) = received_envelope {
                            return Some(received_msg);
                        }
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

/// Test publishing multiple events to sharded topics
#[tokio::test]
async fn test_kafka_publish_multiple_events() {
    let kafka_brokers =
        std::env::var("KAFKA__BROKERS").unwrap_or_else(|_| "localhost:9093".to_string());

    let _test_db = TestDb::new().await;
    let kafka_producer = create_kafka_producer(&kafka_brokers);

    // Use the same channel for all messages (will go to same shard)
    let channel_id = ChannelId::new();

    // Publish multiple events
    for i in 0..5 {
        let message = Message {
            id: MessageId::new_time_based(),
            channel_id,
            user_id: UserId(uuid::Uuid::new_v4()),
            content: MessageContent::new(format!("Test message {}", i)).unwrap(),
            timestamp: chrono::Utc::now(),
        };

        let event = MessageSentEvent::new(&message);
        let key = event.message_id.to_string();

        // Wrap in serializable message envelope
        let message_envelope = MessageSentMessage::from(&event);
        let envelope = ChatEventMessage::MessageSent(message_envelope);

        let result = kafka_producer
            .publish_event(channel_id, &key, &envelope)
            .await;

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
    let message = Message {
        id: MessageId::new_time_based(),
        channel_id,
        user_id: UserId(uuid::Uuid::new_v4()),
        content: MessageContent::new("Test message".to_string()).unwrap(),
        timestamp: chrono::Utc::now(),
    };

    let event = MessageSentEvent::new(&message);
    let key = event.message_id.to_string();

    // Wrap in serializable message envelope
    let message_envelope = MessageSentMessage::from(&event);
    let envelope = ChatEventMessage::MessageSent(message_envelope);

    // This should fail with timeout or connection error
    let result = timeout(
        Duration::from_secs(7),
        kafka_producer.publish_event(channel_id, &key, &envelope),
    )
    .await;

    // Either timeout or error from Kafka
    assert!(
        result.is_err() || result.unwrap().is_err(),
        "Expected error when publishing to invalid broker"
    );
}

/// Test that different channels map to different shards
#[tokio::test]
async fn test_kafka_sharding_distribution() {
    use std::collections::HashSet;

    use chat_service::outbound::events::topic::TopicSharder;

    let sharder = TopicSharder::new(16, "chat.messages").unwrap();

    // Create 100 different channels and track which shards they map to
    let mut shards_used = HashSet::new();
    for _ in 0..100 {
        let channel_id = ChannelId::new();
        let shard = sharder.get_shard_for_channel(channel_id);
        shards_used.insert(shard);
    }

    // With 100 random channels and 16 shards, we should use most shards
    // (statistically very unlikely to use fewer than 10 shards)
    assert!(
        shards_used.len() >= 10,
        "Expected to use at least 10 shards, but only used {}",
        shards_used.len()
    );
}

/// Test that the same channel always maps to the same shard (consistency)
#[tokio::test]
async fn test_kafka_sharding_consistency() {
    use chat_service::outbound::events::topic::TopicSharder;

    let sharder = TopicSharder::new(16, "chat.messages").unwrap();

    let channel_id = ChannelId::new();

    // Get the shard multiple times
    let shard1 = sharder.get_shard_for_channel(channel_id);
    let shard2 = sharder.get_shard_for_channel(channel_id);
    let shard3 = sharder.get_shard_for_channel(channel_id);

    // All should be the same
    assert_eq!(shard1, shard2);
    assert_eq!(shard2, shard3);
}
