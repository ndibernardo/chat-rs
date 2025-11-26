use std::time::Duration;

use async_trait::async_trait;
use rdkafka::config::ClientConfig;
use rdkafka::producer::FutureProducer;
use rdkafka::producer::FutureRecord;
use rdkafka::util::Timeout;
use serde::Serialize;
use thiserror::Error;

use crate::config::Config;
use crate::domain::user::events::UserCreatedEvent;
use crate::domain::user::events::UserDeletedEvent;
use crate::domain::user::events::UserUpdatedEvent;
use crate::outbound::events::messages::UserEventMessage;
use crate::user::errors::EventPublisherError;
use crate::user::ports::EventPublisher;

#[derive(Debug, Error)]
pub enum KafkaProducerError {
    #[error("Failed to send message to Kafka: {0}")]
    SendError(String),

    #[error("Failed to serialize message: {0}")]
    SerializationError(String),
}

impl From<KafkaProducerError> for EventPublisherError {
    fn from(err: KafkaProducerError) -> Self {
        match err {
            KafkaProducerError::SerializationError(msg) => {
                EventPublisherError::SerializationFailed(msg)
            }
            KafkaProducerError::SendError(msg) => EventPublisherError::PublishFailed(msg),
        }
    }
}

pub struct KafkaEventProducer {
    producer: FutureProducer,
    topic: String,
    timeout: Duration,
}

impl KafkaEventProducer {
    /// Create a new Kafka event producer with "at least once" delivery semantics
    ///
    /// # Arguments
    /// * `config` - Application configuration
    ///
    /// # Notes:
    /// - `acks=all`: Wait for all in-sync replicas to acknowledge
    /// - `enable.idempotence=true`: Prevents duplicate messages during retries
    /// - `max.in.flight.requests.per.connection=5`: Allows pipelining with ordering guarantees
    /// - `retry.backoff.ms=100`: Backoff between retry attempts
    pub fn new(config: &Config) -> Result<Self, anyhow::Error> {
        tracing::info!(
            "Initializing Kafka producer for user events: brokers={}, topic={}",
            &config.kafka.brokers,
            &config.kafka.topic
        );

        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka.brokers)
            .set("message.timeout.ms", "30000")
            .set("queue.buffering.max.messages", "10000")
            .set("queue.buffering.max.kbytes", "1048576")
            .set("batch.num.messages", "100")
            .set("compression.type", "gzip")
            .set("enable.idempotence", "true")
            .set("acks", "all")
            .set("retries", "10")
            .set("max.in.flight.requests.per.connection", "5")
            .set("retry.backoff.ms", "100")
            .create()?;

        tracing::info!("Kafka producer initialized successfully");

        Ok(Self {
            producer,
            topic: config.kafka.topic.to_string(),
            timeout: Duration::from_secs(30),
        })
    }

    /// Publish a domain event to Kafka with at-least-once delivery semantics
    ///
    /// The event will be partitioned by user_id to ensure ordering for the same user.
    /// Kafka producer handles retries automatically based on configuration.
    async fn publish<T: Serialize>(
        &self,
        user_id: &str,
        event: &T,
    ) -> Result<(), KafkaProducerError> {
        let payload = serde_json::to_string(event)
            .map_err(|e| KafkaProducerError::SerializationError(e.to_string()))?;

        tracing::debug!(
            "Publishing event to topic '{}' (user_id: {})",
            self.topic,
            user_id
        );

        let record = FutureRecord::to(&self.topic)
            .key(user_id) // Partition by user_id for ordering
            .payload(&payload);

        // Send to Kafka - producer will handle retries automatically with at-least-once semantics
        self.producer
            .send(record, Timeout::After(self.timeout))
            .await
            .map(|_| {
                tracing::debug!(
                    "Event published successfully to topic '{}' for user {}",
                    self.topic,
                    user_id
                );
            })
            .map_err(|(err, _)| {
                tracing::error!(
                    "Failed to publish event to Kafka after all retries: {}",
                    err
                );
                KafkaProducerError::SendError(err.to_string())
            })
    }
}

#[async_trait]
impl EventPublisher for KafkaEventProducer {
    async fn publish_user_created(
        &self,
        event: &UserCreatedEvent,
    ) -> Result<(), EventPublisherError> {
        // Convert domain event to serializable message
        let message: UserEventMessage = event.clone().into();

        self.publish(&event.user_id, &message).await.map_err(|e| {
            // Log error but don't propagate - eventual consistency
            tracing::error!(
                "Failed to publish UserCreated event for user {}: {}",
                event.user_id,
                e
            );
            e.into()
        })
    }

    async fn publish_user_updated(
        &self,
        event: &UserUpdatedEvent,
    ) -> Result<(), EventPublisherError> {
        // Convert domain event to serializable message
        let message: UserEventMessage = event.clone().into();

        self.publish(&event.user_id, &message).await.map_err(|e| {
            tracing::error!(
                "Failed to publish UserUpdated event for user {}: {}",
                event.user_id,
                e
            );
            e.into()
        })
    }

    async fn publish_user_deleted(
        &self,
        event: &UserDeletedEvent,
    ) -> Result<(), EventPublisherError> {
        // Convert domain event to serializable message
        let message: UserEventMessage = event.clone().into();

        self.publish(&event.user_id, &message).await.map_err(|e| {
            tracing::error!(
                "Failed to publish UserDeleted event for user {}: {}",
                event.user_id,
                e
            );
            e.into()
        })
    }
}
