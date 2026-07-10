use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use rdkafka::Message;
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
use crate::domain::channel::ports::ChannelRepository;
use crate::domain::message::ports::MessageRepository;
use crate::domain::user::events::UserDeletedEvent;
use crate::domain::user::events::UserEvent;
use crate::domain::user::models::UserId;
use crate::outbound::kafka::envelope::SCHEMA_USER_V1;
use crate::outbound::kafka::envelope::decode_envelope;
use crate::outbound::kafka::messages::UserEventMessage;

#[derive(Debug, Error)]
enum MessageProcessingError {
    #[error("Kafka consumer error: {0}")]
    KafkaError(#[from] KafkaError),
}

#[derive(Debug, Error)]
enum CleanupError {
    #[error("invalid user id in user_deleted event: {0}")]
    InvalidUserId(String),

    #[error("failed to delete user messages: {0}")]
    Messages(String),

    #[error("failed to remove user memberships: {0}")]
    Memberships(String),

    #[error("failed to deactivate direct channels: {0}")]
    DirectChannels(String),
}

/// Erases a deleted user's footprint from this service on `user_deleted`.
///
/// Shared consumer group on the user-events topic, separate from the
/// replica consumer's group: replica upkeep is cheap and latency-sensitive,
/// cleanup pages through a user's entire message history — one must never
/// queue behind the other.
pub struct CleanupConsumer<M: MessageRepository, C: ChannelRepository> {
    consumer: StreamConsumer<AssignmentTracker>,
    message_repository: Arc<M>,
    channel_repository: Arc<C>,
    assignment_tracker: AssignmentTracker,
    dlq: DeadLetterQueue,
    max_attempts: u32,
}

impl<M: MessageRepository, C: ChannelRepository> CleanupConsumer<M, C> {
    /// # Arguments
    /// * `config` - Application configuration
    /// * `message_repository` - Cassandra-backed message store to erase from
    /// * `channel_repository` - Postgres-backed channel store to erase from
    pub fn new(
        config: &Config,
        message_repository: Arc<M>,
        channel_repository: Arc<C>,
    ) -> Result<Self, anyhow::Error> {
        tracing::info!(
            "Initializing deleted-user cleanup consumer: brokers={}, group_id={}, topic={}",
            &config.kafka.brokers,
            &config.kafka.cleanup.group_id,
            &config.kafka.user_events.topic
        );

        // Shared group: every instance advances the same offsets, so each
        // deletion is cleaned up exactly once regardless of replica count.
        let instance_id = resolve_instance_id(config);
        let assignment_tracker = AssignmentTracker::new();

        let consumer: StreamConsumer<AssignmentTracker> =
            base_consumer_config(config, &instance_id)
                .set("group.id", &config.kafka.cleanup.group_id)
                // Committed manually, only after every cleanup step succeeded:
                // auto-commit's at-most-once semantics would let a crashed
                // handler permanently skip a deletion — leaked personal data.
                .set("enable.auto.commit", "false")
                .set("auto.offset.reset", "earliest")
                .set("session.timeout.ms", "45000")
                .create_with_context(assignment_tracker.clone())?;

        consumer.subscribe(&[&config.kafka.user_events.topic])?;

        tracing::info!(
            "Deleted-user cleanup consumer initialized and subscribed to '{}'",
            &config.kafka.user_events.topic
        );

        let dlq = DeadLetterQueue::new(config, &config.kafka.user_events.topic)?;

        Ok(Self {
            consumer,
            message_repository,
            channel_repository,
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
        tracing::info!("Starting deleted-user cleanup consumer loop");

        let mut message_stream = self.consumer.stream();

        loop {
            let result = tokio::select! {
                _ = cancellation_token.cancelled() => {
                    tracing::info!("Cancellation requested, stopping cleanup consumer loop");
                    break;
                }
                result = message_stream.next() => result,
            };

            let Some(result) = result else {
                tracing::warn!("Cleanup consumer stream ended");
                break;
            };

            if let Err(error) = self.process_message(result).await {
                web::metrics::record_kafka_consumed(
                    web::metrics::ConsumerKind::Cleanup,
                    web::metrics::Outcome::Error,
                );
                tracing::error!("Error processing user event for cleanup: {}", error);

                if matches!(error, MessageProcessingError::KafkaError(_)) {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            } else {
                web::metrics::record_kafka_consumed(
                    web::metrics::ConsumerKind::Cleanup,
                    web::metrics::Outcome::Success,
                );
            }
        }

        tracing::info!("Deleted-user cleanup consumer loop ended");
    }

    /// Every non-`UserDeleted` variant on this topic is the replica
    /// consumer's business, not cleanup's, so it commits immediately
    /// without acting.
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

        let event_message = match decode_envelope::<UserEventMessage>(payload, SCHEMA_USER_V1) {
            Ok(event_message) => event_message,
            Err(decode_error) => {
                self.send_to_dlq(&message, payload, &decode_error.to_string())
                    .await;
                return self.commit(&message);
            }
        };

        let event = match UserEvent::try_from(event_message) {
            Ok(event) => event,
            Err(conversion_error) => {
                self.send_to_dlq(&message, payload, &conversion_error).await;
                return self.commit(&message);
            }
        };

        let deleted = match event {
            UserEvent::UserDeleted(deleted) => deleted,
            UserEvent::UserCreated(_) | UserEvent::UserUpdated(_) => {
                return self.commit(&message);
            }
        };

        let mut attempt = 0u32;
        loop {
            match self.cleanup(&deleted).await {
                Ok(()) => break,
                // An unparseable user id fails identically on every retry —
                // deterministic poison, straight to the DLQ.
                Err(err @ CleanupError::InvalidUserId(_)) => {
                    self.send_to_dlq(&message, payload, &err.to_string()).await;
                    break;
                }
                Err(err) => {
                    attempt += 1;
                    if attempt >= self.max_attempts {
                        tracing::error!(
                            attempts = attempt,
                            error = %err,
                            "Exhausted retries cleaning up deleted user, sending to DLQ"
                        );
                        self.send_to_dlq(
                            &message,
                            payload,
                            &format!("Exhausted {attempt} attempts: {err}"),
                        )
                        .await;
                        break;
                    }
                    tracing::warn!(attempt, error = %err, "Transient error cleaning up deleted user, retrying");
                    tokio::time::sleep(Duration::from_millis(200 * u64::from(attempt))).await;
                }
            }
        }

        self.commit(&message)
    }

    /// Commits the offset for `message`. Called unconditionally once a
    /// message has been either handled or terminally dead-lettered, so the
    /// consumer never gets stuck redelivering the same message.
    fn commit(&self, message: &BorrowedMessage<'_>) -> Result<(), MessageProcessingError> {
        self.consumer
            .commit_message(message, CommitMode::Async)
            .map_err(MessageProcessingError::KafkaError)
    }

    async fn send_to_dlq(&self, message: &BorrowedMessage<'_>, payload: &[u8], reason: &str) {
        tracing::warn!(reason, "Sending user event message to dead-letter queue");
        self.dlq.publish(message.key(), payload, reason).await;
        web::metrics::record_kafka_dlq(web::metrics::ConsumerKind::Cleanup);
    }

    /// Erase the user's footprint. Every step is idempotent and they run in
    /// a fixed order — message history first (found via the messages_by_user
    /// index), then membership rows, then direct-channel deactivation (found
    /// via `direct_channel_keys`, which membership removal doesn't touch) —
    /// so a retry after a partial failure resumes safely from the top.
    async fn cleanup(&self, event: &UserDeletedEvent) -> Result<(), CleanupError> {
        tracing::info!("Cleaning up deleted user {}", event.user_id);

        let user_id = UserId::from_string(&event.user_id)
            .map_err(|e| CleanupError::InvalidUserId(e.to_string()))?;

        self.message_repository
            .delete_all_by_user(user_id)
            .await
            .map_err(|e| CleanupError::Messages(e.to_string()))?;

        self.channel_repository
            .remove_user_memberships(user_id)
            .await
            .map_err(|e| CleanupError::Memberships(e.to_string()))?;

        self.channel_repository
            .deactivate_direct_channels_of(user_id, event.deleted_at)
            .await
            .map_err(|e| CleanupError::DirectChannels(e.to_string()))?;

        tracing::info!("Deleted user {} cleaned up", event.user_id);

        Ok(())
    }
}
