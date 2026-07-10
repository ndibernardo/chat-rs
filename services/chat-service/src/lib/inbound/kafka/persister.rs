use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use rdkafka::Message as _;
use rdkafka::consumer::CommitMode;
use rdkafka::consumer::Consumer;
use rdkafka::consumer::StreamConsumer;
use rdkafka::error::KafkaError;
use rdkafka::message::BorrowedMessage;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use super::context::AssignmentTracker;
use super::dlq::DeadLetterQueue;
use super::instance::base_consumer_config;
use super::instance::resolve_instance_id;
use crate::config::Config;
use crate::domain::channel::models::ChannelId;
use crate::domain::message::errors::MessageError;
use crate::domain::message::models::Message;
use crate::domain::message::models::MessageContent;
use crate::domain::message::models::MessageId;
use crate::domain::message::ports::MessageRepository;
use crate::domain::user::models::UserId;
use crate::outbound::kafka::envelope::SCHEMA_CHAT_V1;
use crate::outbound::kafka::envelope::decode_envelope;
use crate::outbound::kafka::messages::ChatEventMessage;
use crate::outbound::kafka::messages::MessageSentMessage;

#[derive(Debug, Error)]
enum MessageProcessingError {
    #[error("Kafka consumer error: {0}")]
    KafkaError(#[from] KafkaError),
}

/// Writes `MessageSent` events from the single chat topic into Cassandra.
///
/// This is the Kafka-first message path's durable write: `send_message`
/// only waits for the broker's ack, so history persistence happens here,
/// asynchronously, on a shared consumer group (every instance advances the
/// same offsets — unlike the per-instance broadcast consumer, duplicating
/// this write across replicas would double-insert every message).
pub struct MessagePersister<R: MessageRepository> {
    consumer: StreamConsumer<AssignmentTracker>,
    message_repository: Arc<R>,
    assignment_tracker: AssignmentTracker,
    dlq: DeadLetterQueue,
    max_attempts: u32,
}

impl<R: MessageRepository> MessagePersister<R> {
    /// # Arguments
    /// * `config` - Application configuration
    /// * `message_repository` - Cassandra-backed message store to persist into
    pub fn new(config: &Config, message_repository: Arc<R>) -> Result<Self, anyhow::Error> {
        tracing::info!(
            "Initializing message persister: brokers={}, group_id={}, topic={}",
            &config.kafka.brokers,
            &config.kafka.persister.group_id,
            &config.kafka.messages_topic
        );

        // Shared group: every instance advances the same offsets, so a
        // message is persisted exactly once regardless of how many
        // instances are running.
        let instance_id = resolve_instance_id(config);
        let assignment_tracker = AssignmentTracker::new();

        let consumer: StreamConsumer<AssignmentTracker> =
            base_consumer_config(config, &instance_id)
                .set("group.id", &config.kafka.persister.group_id)
                // Committed manually, only after a successful Cassandra write: this
                // consumer is the durable write path for message history, so
                // auto-commit's at-most-once semantics would let a crashed handler
                // permanently skip a message.
                .set("enable.auto.commit", "false")
                .set("auto.offset.reset", "earliest")
                .set("session.timeout.ms", "45000")
                .create_with_context(assignment_tracker.clone())?;

        consumer.subscribe(&[&config.kafka.messages_topic])?;

        tracing::info!(
            "Message persister initialized and subscribed to '{}'",
            &config.kafka.messages_topic
        );

        let dlq = DeadLetterQueue::new(config, &config.kafka.messages_topic)?;

        Ok(Self {
            consumer,
            message_repository,
            assignment_tracker,
            dlq,
            max_attempts: config.kafka.dlq.max_attempts,
        })
    }

    /// Handle for the readiness check: whether this consumer currently holds
    /// a partition assignment.
    pub fn assignment_tracker(&self) -> AssignmentTracker {
        self.assignment_tracker.clone()
    }

    /// Runs until `cancellation_token` fires, so a graceful shutdown can
    /// stop this loop cooperatively instead of aborting it mid-poll.
    pub async fn start_consuming(self, cancellation_token: CancellationToken) {
        tracing::info!("Starting message persister loop");

        let mut message_stream = self.consumer.stream();

        loop {
            let result = tokio::select! {
                _ = cancellation_token.cancelled() => {
                    tracing::info!("Cancellation requested, stopping message persister loop");
                    break;
                }
                result = message_stream.next() => result,
            };

            let Some(result) = result else {
                tracing::warn!("Message persister stream ended");
                break;
            };

            if let Err(error) = self.process_message(result).await {
                web::metrics::record_kafka_consumed(
                    web::metrics::ConsumerKind::Persister,
                    web::metrics::Outcome::Error,
                );
                tracing::error!("Error processing message for persistence: {}", error);

                if matches!(error, MessageProcessingError::KafkaError(_)) {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            } else {
                web::metrics::record_kafka_consumed(
                    web::metrics::ConsumerKind::Persister,
                    web::metrics::Outcome::Success,
                );
            }
        }

        tracing::info!("Message persister loop ended");
    }

    /// Every non-`MessageSent` variant on this topic (channel lifecycle
    /// events) is irrelevant to persistence, so it commits immediately
    /// without a write.
    async fn process_message(
        &self,
        result: Result<BorrowedMessage<'_>, KafkaError>,
    ) -> Result<(), MessageProcessingError> {
        let message = result?;

        let Some(payload) = message.payload() else {
            self.send_to_dlq(&message, &[], "Message has no payload")
                .await;
            return self.commit(&message);
        };

        let event = match decode_envelope::<ChatEventMessage>(payload, SCHEMA_CHAT_V1) {
            Ok(event) => event,
            Err(decode_error) => {
                self.send_to_dlq(&message, payload, &decode_error.to_string())
                    .await;
                return self.commit(&message);
            }
        };

        let ChatEventMessage::MessageSent(sent) = event else {
            return self.commit(&message);
        };

        let mut attempt = 0u32;
        loop {
            match self.persist(sent.clone()).await {
                Ok(()) => break,
                Err(err) => {
                    attempt += 1;
                    if attempt >= self.max_attempts {
                        tracing::error!(
                            attempts = attempt,
                            error = %err,
                            "Exhausted retries persisting message, sending to DLQ"
                        );
                        self.send_to_dlq(
                            &message,
                            payload,
                            &format!("Exhausted {attempt} attempts: {err}"),
                        )
                        .await;
                        break;
                    }
                    tracing::warn!(attempt, error = %err, "Transient error persisting message, retrying");
                    tokio::time::sleep(Duration::from_millis(200 * u64::from(attempt))).await;
                }
            }
        }

        self.commit(&message)
    }

    /// Commits the offset for `message`. Called unconditionally once a
    /// message has been either persisted or terminally dead-lettered, so the
    /// consumer never gets stuck redelivering the same message.
    fn commit(&self, message: &BorrowedMessage<'_>) -> Result<(), MessageProcessingError> {
        self.consumer
            .commit_message(message, CommitMode::Async)
            .map_err(MessageProcessingError::KafkaError)
    }

    async fn send_to_dlq(&self, message: &BorrowedMessage<'_>, payload: &[u8], reason: &str) {
        tracing::warn!(reason, "Sending chat message to dead-letter queue");
        self.dlq.publish(message.key(), payload, reason).await;
        web::metrics::record_kafka_dlq(web::metrics::ConsumerKind::Persister);
    }

    /// Reconstructs the domain message from the wire event and writes it to
    /// Cassandra. Idempotent: `message_id` is deterministic (minted once by
    /// the sender), so redelivering the same event overwrites the same row.
    async fn persist(&self, event: MessageSentMessage) -> Result<(), MessageError> {
        let channel_id = ChannelId::from_string(&event.channel_id)?;
        let message_id = MessageId::from_string(&event.message_id)?;
        let user_id = UserId::from_string(&event.user_id)?;
        let content = MessageContent::new(event.content)?;

        let message =
            Message::from_parts(message_id, channel_id, user_id, content, event.timestamp);

        self.message_repository.create(message).await?;
        Ok(())
    }
}
