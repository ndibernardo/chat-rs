use std::sync::Arc;

use chrono::Utc;
use futures::StreamExt;
use rdkafka::consumer::Consumer;
use rdkafka::consumer::StreamConsumer;
use rdkafka::error::KafkaError;
use rdkafka::ClientConfig;
use rdkafka::Message;
use thiserror::Error;

use super::messages::UserEventMessage;
use crate::config::Config;
use crate::domain::user::events::UserCreatedEvent;
use crate::domain::user::events::UserDeletedEvent;
use crate::domain::user::events::UserEvent;
use crate::domain::user::events::UserUpdatedEvent;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::domain::user::models::Username;
use crate::domain::user::ports::UserReplicaRepository;

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
    consumer: StreamConsumer,
    user_replica_repository: Arc<R>,
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
            &config.kafka.group_id,
            &config.kafka.user_events.topic
        );

        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka.brokers)
            .set("group.id", &config.kafka.group_id)
            .set("enable.auto.commit", "true")
            .set("auto.commit.interval.ms", "5000")
            .set("auto.offset.reset", "earliest") // Process all user events from beginning
            .set("session.timeout.ms", "30000")
            .set("enable.partition.eof", "false")
            .create()?;

        // Subscribe to user-events topic
        consumer.subscribe(&[&config.kafka.user_events.topic])?;

        tracing::info!(
            "User events consumer initialized and subscribed to '{}'",
            &config.kafka.user_events.topic
        );

        Ok(Self {
            consumer,
            user_replica_repository,
        })
    }

    /// Start consuming user events from Kafka
    ///
    /// This is a long-running task that should be spawned in a separate tokio task
    pub async fn start_consuming(self) {
        tracing::info!("Starting user events consumer loop");

        let mut message_stream = self.consumer.stream();

        while let Some(result) = message_stream.next().await {
            if let Err(error) = self.process_message(result).await {
                tracing::error!("Error processing user event: {}", error);

                // Add backoff on Kafka errors to avoid tight error loops
                if matches!(error, MessageProcessingError::KafkaError(_)) {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }

        tracing::warn!("User events consumer loop ended");
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
            .map_err(MessageProcessingError::HandlingError)
    }

    /// Handle a user event by updating the local replica
    async fn handle_event(&self, event: UserEvent) -> Result<(), String> {
        match event {
            UserEvent::UserCreated(created_event) => self.handle_user_created(created_event).await,
            UserEvent::UserUpdated(updated_event) => self.handle_user_updated(updated_event).await,
            UserEvent::UserDeleted(deleted_event) => self.handle_user_deleted(deleted_event).await,
        }
    }

    /// Handle UserCreated event - insert user into replica
    async fn handle_user_created(&self, event: UserCreatedEvent) -> Result<(), String> {
        tracing::info!("Handling UserCreated event for user {}", event.user_id);

        let user_id = UserId::from_string(&event.user_id)
            .map_err(|error| format!("Invalid user_id in UserCreated event: {}", error))?;

        let username = Username::new(event.username.clone())
            .map_err(|error| format!("Invalid username in UserCreated event: {}", error))?;

        let user = User {
            id: user_id,
            username,
            created_at: event.created_at,
            updated_at: event.created_at, // Same as created_at for new users
        };

        self.user_replica_repository.upsert(user).await?;

        tracing::info!(
            "User {} ({}) added to replica",
            event.user_id,
            event.username
        );

        Ok(())
    }

    /// Handle UserUpdated event - update user in replica
    async fn handle_user_updated(&self, event: UserUpdatedEvent) -> Result<(), String> {
        tracing::info!("Handling UserUpdated event for user {}", event.user_id);

        let user_id = UserId::from_string(&event.user_id)
            .map_err(|error| format!("Invalid user_id in UserUpdated event: {}", error))?;

        // Get existing user to preserve created_at
        let existing_user = self.user_replica_repository.get(user_id).await?;

        let created_at = existing_user
            .map(|user| user.created_at)
            .unwrap_or_else(|| {
                tracing::warn!(
                    "User {} not found in replica during update, using current time for created_at",
                    event.user_id
                );
                Utc::now()
            });

        let username = Username::new(event.username.clone())
            .map_err(|error| format!("Invalid username in UserUpdated event: {}", error))?;

        let user = User {
            id: user_id,
            username,
            created_at,
            updated_at: event.updated_at,
        };

        self.user_replica_repository.upsert(user).await?;

        tracing::info!(
            "User {} ({}) updated in replica",
            event.user_id,
            event.username
        );

        Ok(())
    }

    /// Handle UserDeleted event - remove user from replica
    async fn handle_user_deleted(&self, event: UserDeletedEvent) -> Result<(), String> {
        tracing::info!("Handling UserDeleted event for user {}", event.user_id);

        let user_id = UserId::from_string(&event.user_id)
            .map_err(|error| format!("Invalid user_id in UserDeleted event: {}", error))?;

        self.user_replica_repository.delete(user_id).await?;

        tracing::info!("User {} deleted from replica", event.user_id);

        Ok(())
    }
}
