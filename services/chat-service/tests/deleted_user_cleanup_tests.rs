mod common;

use std::sync::Arc;
use std::time::Duration;

use chat_service::config::CassandraConfig;
use chat_service::config::CleanupConfig;
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
use chat_service::domain::channel::events::ChannelCreatedEvent;
use chat_service::domain::channel::models::Channel;
use chat_service::domain::channel::models::ChannelName;
use chat_service::domain::channel::ports::ChannelRepository as _;
use chat_service::domain::message::models::Limit;
use chat_service::domain::message::models::Message;
use chat_service::domain::message::models::MessageContent;
use chat_service::domain::message::ports::MessageRepository as _;
use chat_service::domain::user::models::UserId;
use chat_service::inbound::kafka::CleanupConsumer;
use chat_service::outbound::kafka::envelope::Envelope;
use chat_service::outbound::kafka::envelope::SCHEMA_USER_V1;
use chat_service::outbound::kafka::messages::UserDeletedMessage;
use chat_service::outbound::kafka::messages::UserEventMessage;
use chat_service::outbound::postgres::ChannelRepository;
use chat_service::outbound::scylla::MessageRepository as ScyllaMessageRepository;
use common::TestDb;
use rdkafka::ClientConfig;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use tokio_util::sync::CancellationToken;

const POLL_DEADLINE: Duration = Duration::from_secs(15);

fn cleanup_test_config(test_db: &TestDb, user_events_topic: String) -> Config {
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
            messages_topic: "chat.messages".to_string(),
            delivery_timeout_ms: 10_000,
            instance_id: Some(format!("test-instance-{}", uuid::Uuid::new_v4())),
            user_events: UserEventsConfig {
                topic: user_events_topic,
                group_id: format!("test-user-events-{}", uuid::Uuid::new_v4()),
            },
            dlq: DlqConfig::default(),
            persister: PersisterConfig::default(),
            cleanup: CleanupConfig {
                group_id: format!("test-cleanup-{}", uuid::Uuid::new_v4()),
            },
        },
        websocket: WebsocketConfig::default(),
        shutdown: ShutdownConfig::default(),
        outbox: OutboxConfig::default(),
    }
}

/// Publishes a `user_deleted` envelope to the user-events topic, exactly as
/// user-service's outbox relay would.
async fn publish_user_deleted(
    config: &Config,
    user_id: UserId,
    deleted_at: chrono::DateTime<chrono::Utc>,
) {
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &config.kafka.brokers)
        .set("message.timeout.ms", "10000")
        .create()
        .expect("Failed to create Kafka producer");

    let event = UserEventMessage::UserDeleted(UserDeletedMessage {
        event_id: uuid::Uuid::new_v4().to_string(),
        user_id: user_id.to_string(),
        deleted_at,
    });
    let payload = serde_json::to_vec(&Envelope::wrap(SCHEMA_USER_V1, event))
        .expect("Failed to serialize user_deleted envelope");

    let key = user_id.to_string();
    producer
        .send(
            FutureRecord::to(&config.kafka.user_events.topic)
                .key(&key)
                .payload(&payload),
            Duration::from_secs(10),
        )
        .await
        .expect("Failed to publish user_deleted event");
}

/// Polls `probe` until it reports done or the deadline passes.
async fn wait_until<F, Fut>(description: &str, mut probe: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + POLL_DEADLINE;
    while !probe().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "Timed out waiting for: {description}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn count_memberships_of(test_db: &TestDb, user_id: UserId) -> i64 {
    sqlx::query_scalar!(
        "SELECT count(*) FROM channel_members WHERE user_id = $1",
        user_id.as_uuid(),
    )
    .fetch_one(&test_db.pg_pool)
    .await
    .expect("Failed to count memberships")
    .unwrap_or(0)
}

async fn deactivated_at_of(
    test_db: &TestDb,
    channel: &Channel,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let channel_id = channel.id();
    sqlx::query_scalar!(
        "SELECT deactivated_at FROM channels WHERE id = $1",
        channel_id.as_uuid(),
    )
    .fetch_one(&test_db.pg_pool)
    .await
    .expect("Failed to read deactivated_at")
}

/// Proves the deleted-user cleanup end to end: seeding a user with message
/// history, a private-channel membership and a direct channel, then
/// publishing a `user_deleted` envelope (exactly as user-service's relay
/// does) erases all three footprints — and redelivering the same event
/// neither wedges the consumer nor overwrites the first deactivation.
#[tokio::test]
async fn cleanup_consumer_erases_deleted_user_and_tolerates_redelivery() {
    // Arrange
    let test_db = TestDb::new().await;
    let user_events_topic = format!("test-user-events-{}", uuid::Uuid::new_v4());
    let config = cleanup_test_config(&test_db, user_events_topic);

    let message_repository = Arc::new(
        ScyllaMessageRepository::new(&config)
            .await
            .expect("Failed to create Scylla message repository"),
    );
    let channel_repository = Arc::new(ChannelRepository::new(
        test_db.pg_pool.clone(),
        "chat.messages".to_string(),
    ));

    let deleted_user = UserId::new();
    let surviving_user = UserId::new();
    let second_deleted_user = UserId::new();

    // A private channel where all three users are members, and a direct
    // channel between the (first) deleted user and the survivor.
    let private_channel = Channel::new_private(
        ChannelName::new("incident-response").unwrap(),
        None,
        vec![deleted_user, second_deleted_user],
        surviving_user,
    );
    channel_repository
        .create(
            private_channel.clone(),
            &ChannelCreatedEvent::new(&private_channel),
        )
        .await
        .expect("Failed to create private channel");

    let direct_channel = Channel::new_direct(deleted_user, surviving_user)
        .expect("Distinct participants form a valid direct channel");
    channel_repository
        .create(
            direct_channel.clone(),
            &ChannelCreatedEvent::new(&direct_channel),
        )
        .await
        .expect("Failed to create direct channel");

    for content in ["We found the root cause", "Rolling back the deploy"] {
        message_repository
            .create(Message::new(
                private_channel.id(),
                deleted_user,
                MessageContent::new(content.to_string()).unwrap(),
            ))
            .await
            .expect("Failed to seed deleted user's message");
    }
    message_repository
        .create(Message::new(
            private_channel.id(),
            surviving_user,
            MessageContent::new("Postmortem doc is up".to_string()).unwrap(),
        ))
        .await
        .expect("Failed to seed surviving user's message");

    let consumer = CleanupConsumer::new(
        &config,
        Arc::clone(&message_repository),
        Arc::clone(&channel_repository),
    )
    .expect("Failed to create cleanup consumer");
    let cancellation = CancellationToken::new();
    let consumer_token = cancellation.clone();
    let consumer_handle = tokio::spawn(async move {
        consumer.start_consuming(consumer_token).await;
    });

    // Act: first delivery.
    let deleted_at = chrono::Utc::now();
    publish_user_deleted(&config, deleted_user, deleted_at).await;

    // Assert: all three footprints erased.
    let limit = Limit::new(10).unwrap();
    wait_until("deleted user's messages to be erased", || async {
        message_repository
            .find_by_user(deleted_user, limit)
            .await
            .expect("Failed to query messages_by_user")
            .is_empty()
    })
    .await;
    wait_until("deleted user's memberships to be removed", || async {
        count_memberships_of(&test_db, deleted_user).await == 0
    })
    .await;
    wait_until("direct channel to be deactivated", || async {
        deactivated_at_of(&test_db, &direct_channel).await.is_some()
    })
    .await;

    let channel_messages = message_repository
        .find_by_channel(private_channel.id(), limit, None)
        .await
        .expect("Failed to query messages_by_channel");
    assert!(
        channel_messages.iter().all(|m| m.user_id() != deleted_user),
        "messages_by_channel still holds the deleted user's messages"
    );
    assert_eq!(
        channel_messages.len(),
        1,
        "surviving user's message must remain in channel history"
    );
    assert_eq!(count_memberships_of(&test_db, surviving_user).await, 1);
    assert_eq!(
        deactivated_at_of(&test_db, &private_channel).await,
        None,
        "non-direct channels must never be deactivated"
    );
    let first_deactivation = deactivated_at_of(&test_db, &direct_channel)
        .await
        .expect("Direct channel was just deactivated");

    // Act: redeliver the same event with a later timestamp, then delete a
    // second user. The second cleanup completing proves the redelivery was
    // consumed without wedging the consumer.
    publish_user_deleted(
        &config,
        deleted_user,
        deleted_at + chrono::Duration::seconds(60),
    )
    .await;
    publish_user_deleted(&config, second_deleted_user, chrono::Utc::now()).await;

    wait_until(
        "second deleted user's memberships to be removed",
        || async { count_memberships_of(&test_db, second_deleted_user).await == 0 },
    )
    .await;

    // Assert: idempotent redelivery kept the original deactivation instant.
    assert_eq!(
        deactivated_at_of(&test_db, &direct_channel).await,
        Some(first_deactivation),
        "redelivery must not overwrite the original deactivation timestamp"
    );

    cancellation.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), consumer_handle).await;
}
