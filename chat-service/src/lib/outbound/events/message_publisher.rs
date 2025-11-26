/// Kafka adapter implementing MessageEventPublisher port.
///
/// Converts domain events to infrastructure messages and publishes to Kafka.
use std::sync::Arc;

use async_trait::async_trait;

use super::messages::ChatEventMessage;
use super::messages::MessageDeletedMessage;
use super::messages::MessageSentMessage;
use super::producer::KafkaEventProducer;
use crate::domain::errors::EventPublisherError;
use crate::domain::message::events::MessageDeletedEvent;
use crate::domain::message::events::MessageSentEvent;
use crate::domain::message::ports::MessageEventPublisher;

/// Kafka implementation of MessageEventPublisher.
///
/// Publishes message domain events to Kafka topics using the event producer.
pub struct KafkaMessageEventPublisher {
    producer: Arc<KafkaEventProducer>,
}

impl KafkaMessageEventPublisher {
    /// Create a new Kafka message event publisher.
    ///
    /// # Arguments
    /// * `producer` - Kafka event producer for publishing events
    ///
    /// # Returns
    /// Configured publisher instance
    pub fn new(producer: Arc<KafkaEventProducer>) -> Self {
        Self { producer }
    }
}

#[async_trait]
impl MessageEventPublisher for KafkaMessageEventPublisher {
    async fn publish_message_sent(
        &self,
        event: &MessageSentEvent,
    ) -> Result<(), EventPublisherError> {
        let message = MessageSentMessage::from(event);
        let envelope = ChatEventMessage::MessageSent(message);

        self.producer
            .publish_event(event.channel_id, &event.message_id.to_string(), &envelope)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }

    async fn publish_message_deleted(
        &self,
        event: &MessageDeletedEvent,
    ) -> Result<(), EventPublisherError> {
        let message = MessageDeletedMessage::from(event);

        self.producer
            .publish_event(event.channel_id, &event.message_id.to_string(), &message)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }
}
