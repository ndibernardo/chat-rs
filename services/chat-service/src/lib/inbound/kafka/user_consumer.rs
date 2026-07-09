use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
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
use crate::domain::user::errors::UserError;
use crate::domain::user::events::UserCreatedEvent;
use crate::domain::user::events::UserDeletedEvent;
use crate::domain::user::events::UserEvent;
use crate::domain::user::events::UserUpdatedEvent;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::models::Username;
use crate::domain::user::ports::UserReplicaRepository;
use crate::outbound::kafka::envelope::SCHEMA_USER_V1;
use crate::outbound::kafka::envelope::decode_envelope;
use crate::outbound::kafka::messages::UserEventMessage;

#[derive(Debug, Error)]
enum MessageProcessingError {
    #[error("Kafka consumer error: {0}")]
    KafkaError(#[from] KafkaError),
}

/// Kafka consumer for user events from user-service
///
/// This consumer maintains a local denormalized copy of user data
/// by subscribing to user-events topic and updating the user_replica table
pub struct UserEventsConsumer<R: UserReplicaRepository> {
    consumer: StreamConsumer<AssignmentTracker>,
    user_replica_repository: Arc<R>,
    assignment_tracker: AssignmentTracker,
    dlq: DeadLetterQueue,
    max_attempts: u32,
}

impl<R: UserReplicaRepository> UserEventsConsumer<R> {
    /// Create a new user events consumer
    ///
    /// # Arguments
    /// * `config` - Application configuration
    /// * `user_replica_repository` - Repository for updating local user replica
    pub fn new(config: &Config, user_replica_repository: Arc<R>) -> Result<Self, anyhow::Error> {
        tracing::info!(
            "Initializing user events consumer: brokers={}, group_id={}, topic={}",
            &config.kafka.brokers,
            &config.kafka.user_events.group_id,
            &config.kafka.user_events.topic
        );

        // This is a shared group: every member competes for the same partitions.
        // Static membership (`group.instance.id`, set by `base_consumer_config`)
        // plus a longer session timeout let a restarted pod reclaim its
        // partitions without the coordinator rebalancing the rest of the group
        // out from under it during a rolling deploy.
        let instance_id = resolve_instance_id(config);
        let assignment_tracker = AssignmentTracker::new();

        let consumer: StreamConsumer<AssignmentTracker> =
            base_consumer_config(config, &instance_id)
                .set("group.id", &config.kafka.user_events.group_id)
                // Committed manually, only after a successful replica upsert/delete: this
                // consumer maintains a consistency-sensitive replica, so auto-commit's
                // at-most-once semantics (commit on a timer regardless of processing
                // outcome) would let a failed or crashed handler permanently skip an event.
                .set("enable.auto.commit", "false")
                .set("auto.offset.reset", "earliest") // Process all user events from beginning
                .set("session.timeout.ms", "45000")
                .create_with_context(assignment_tracker.clone())?;

        // Subscribe to user-events topic
        consumer.subscribe(&[&config.kafka.user_events.topic])?;

        tracing::info!(
            "User events consumer initialized and subscribed to '{}'",
            &config.kafka.user_events.topic
        );

        let dlq = DeadLetterQueue::new(config, &config.kafka.user_events.topic)?;

        Ok(Self {
            consumer,
            user_replica_repository,
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

    /// Start consuming user events from Kafka. Runs until
    /// `cancellation_token` is cancelled, so a graceful shutdown can stop
    /// this loop cooperatively instead of aborting the task mid-poll.
    ///
    /// This is a long-running task that should be spawned in a separate tokio task
    pub async fn start_consuming(self, cancellation_token: CancellationToken) {
        tracing::info!("Starting user events consumer loop");

        let mut message_stream = self.consumer.stream();

        loop {
            let result = tokio::select! {
                _ = cancellation_token.cancelled() => {
                    tracing::info!("Cancellation requested, stopping user events consumer loop");
                    break;
                }
                result = message_stream.next() => result,
            };

            let Some(result) = result else {
                tracing::warn!("User events stream ended");
                break;
            };

            if let Err(error) = self.process_message(result).await {
                web::metrics::record_kafka_consumed(
                    web::metrics::ConsumerKind::UserEvents,
                    web::metrics::Outcome::Error,
                );
                tracing::error!("Error processing user event: {}", error);

                // Add backoff on Kafka errors to avoid tight error loops
                if matches!(error, MessageProcessingError::KafkaError(_)) {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            } else {
                web::metrics::record_kafka_consumed(
                    web::metrics::ConsumerKind::UserEvents,
                    web::metrics::Outcome::Success,
                );
            }
        }

        tracing::info!("User events consumer loop ended");
    }

    /// Process a single Kafka message.
    ///
    /// A message that doesn't decode as a well-formed `user.v1` envelope is
    /// deterministic poison — retrying the same bytes would fail
    /// identically — so it goes straight to the DLQ. A well-formed message
    /// that fails during `handle_event` (e.g. a transient database error)
    /// is retried with backoff up to `max_attempts` before also going to
    /// the DLQ. Either way this always commits before returning, which is
    /// what keeps a poison message from being redelivered forever: without
    /// that commit, a restart would resume from before it and fail on it
    /// again indefinitely.
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

        tracing::debug!(
            "Received user event: {} ({})",
            event.event_id(),
            event.event_type()
        );

        let mut attempt = 0u32;
        loop {
            match self.handle_event(event.clone()).await {
                Ok(()) => break,
                Err(err) => {
                    attempt += 1;
                    if attempt >= self.max_attempts {
                        tracing::error!(
                            attempts = attempt,
                            error = %err,
                            "Exhausted retries handling user event, sending to DLQ"
                        );
                        self.send_to_dlq(
                            &message,
                            payload,
                            &format!("Exhausted {attempt} attempts: {err}"),
                        )
                        .await;
                        break;
                    }
                    tracing::warn!(attempt, error = %err, "Transient error handling user event, retrying");
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
        web::metrics::record_kafka_dlq(web::metrics::ConsumerKind::UserEvents);
    }

    /// Handle a user event by updating the local replica
    async fn handle_event(&self, event: UserEvent) -> Result<(), UserError> {
        match event {
            UserEvent::UserCreated(created_event) => self.handle_user_created(created_event).await,
            UserEvent::UserUpdated(updated_event) => self.handle_user_updated(updated_event).await,
            UserEvent::UserDeleted(deleted_event) => self.handle_user_deleted(deleted_event).await,
        }
    }

    /// Handle UserCreated event - insert user into replica
    async fn handle_user_created(&self, event: UserCreatedEvent) -> Result<(), UserError> {
        tracing::info!("Handling UserCreated event for user {}", event.user_id);

        let user_id = UserId::from_string(&event.user_id)?;
        let username = Username::new(event.username.clone())?;

        let user = User::new(user_id, username, event.created_at, event.created_at);

        self.user_replica_repository.upsert(user).await?;

        tracing::info!(
            "User {} ({}) added to replica",
            event.user_id,
            event.username
        );

        Ok(())
    }

    /// Handle UserUpdated event - update user in replica
    async fn handle_user_updated(&self, event: UserUpdatedEvent) -> Result<(), UserError> {
        tracing::info!("Handling UserUpdated event for user {}", event.user_id);

        let user_id = UserId::from_string(&event.user_id)?;

        // Get existing user to preserve created_at
        let existing_user = self.user_replica_repository.get(user_id).await?;

        let created_at = existing_user
            .map(|user| user.created_at())
            .unwrap_or_else(|| {
                tracing::warn!(
                    "User {} not found in replica during update, using current time for created_at",
                    event.user_id
                );
                Utc::now()
            });

        let username = Username::new(event.username.clone())?;

        let user = User::new(user_id, username, created_at, event.updated_at);

        self.user_replica_repository.upsert(user).await?;

        tracing::info!(
            "User {} ({}) updated in replica",
            event.user_id,
            event.username
        );

        Ok(())
    }

    /// Handle UserDeleted event - remove user from replica
    async fn handle_user_deleted(&self, event: UserDeletedEvent) -> Result<(), UserError> {
        tracing::info!("Handling UserDeleted event for user {}", event.user_id);

        let user_id = UserId::from_string(&event.user_id)?;

        self.user_replica_repository.delete(user_id).await?;

        tracing::info!("User {} deleted from replica", event.user_id);

        Ok(())
    }
}
