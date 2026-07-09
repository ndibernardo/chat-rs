use std::sync::Arc;

use chrono::Utc;
use futures::StreamExt;
use rdkafka::Message;
use rdkafka::consumer::CommitMode;
use rdkafka::consumer::Consumer;
use rdkafka::consumer::StreamConsumer;
use rdkafka::error::KafkaError;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use super::context::AssignmentTracker;
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
use crate::outbound::kafka::messages::UserEventMessage;

#[derive(Debug, Error)]
enum MessageProcessingError {
    #[error("Kafka consumer error: {0}")]
    KafkaError(#[from] KafkaError),

    #[error("Message has no payload")]
    NoPayload,

    #[error("Failed to decode message payload as UTF-8: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),

    #[error("Failed to deserialize event: {0}")]
    DeserializationError(#[from] serde_json::Error),

    #[error("Failed to handle event: {0}")]
    HandlingError(String),
}

/// Kafka consumer for user events from user-service
///
/// This consumer maintains a local denormalized copy of user data
/// by subscribing to user-events topic and updating the user_replica table
pub struct UserEventsConsumer<R: UserReplicaRepository> {
    consumer: StreamConsumer<AssignmentTracker>,
    user_replica_repository: Arc<R>,
    assignment_tracker: AssignmentTracker,
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

        Ok(Self {
            consumer,
            user_replica_repository,
            assignment_tracker,
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

    /// Process a single Kafka message
    async fn process_message(
        &self,
        result: Result<rdkafka::message::BorrowedMessage<'_>, KafkaError>,
    ) -> Result<(), MessageProcessingError> {
        let message = result?;
        let payload = message.payload().ok_or(MessageProcessingError::NoPayload)?;
        let json_string = std::str::from_utf8(payload)?;
        let event_message = serde_json::from_str::<UserEventMessage>(json_string)?;

        // Convert infrastructure message to domain event
        let event = UserEvent::try_from(event_message)
            .map_err(|e| MessageProcessingError::HandlingError(e.to_string()))?;

        tracing::debug!(
            "Received user event: {} ({})",
            event.event_id(),
            event.event_type()
        );

        self.handle_event(event)
            .await
            .map_err(|e| MessageProcessingError::HandlingError(e.to_string()))?;

        // Commit only now that the replica has actually been updated, so a crash
        // or handling error before this point causes the event to be redelivered
        // instead of silently skipped.
        self.consumer
            .commit_message(&message, CommitMode::Async)
            .map_err(MessageProcessingError::KafkaError)?;

        Ok(())
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
