/// Kafka adapter implementing MessageEventPublisher port.
///
/// Converts domain events to infrastructure messages and publishes to Kafka.
use std::sync::Arc;

use async_trait::async_trait;

use super::envelope::SCHEMA_CHAT_V1;
use super::messages::ChatEventMessage;
use super::messages::MessageDeletedMessage;
use super::messages::MessageSentMessage;
use super::producer::EventProducer;
use crate::domain::errors::EventPublisherError;
use crate::domain::message::events::MessageDeletedEvent;
use crate::domain::message::events::MessageSentEvent;
use crate::domain::message::ports;

/// Kafka implementation of MessageEventPublisher.
///
/// Publishes message domain events to Kafka topics using the event producer.
pub struct MessageEventPublisher {
    producer: Arc<EventProducer>,
}

impl MessageEventPublisher {
    /// Create a new Kafka message event publisher.
    ///
    /// # Arguments
    /// * `producer` - Kafka event producer for publishing events
    ///
    /// # Returns
    /// Configured publisher instance
    pub fn new(producer: Arc<EventProducer>) -> Self {
        Self { producer }
    }
}

#[async_trait]
impl ports::MessageEventPublisher for MessageEventPublisher {
    async fn publish_message_sent(
        &self,
        event: &MessageSentEvent,
    ) -> Result<(), EventPublisherError> {
        let message = ChatEventMessage::MessageSent(MessageSentMessage::from(event));

        self.producer
            .publish_event(event.channel_id, SCHEMA_CHAT_V1, message)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }

    async fn publish_message_deleted(
        &self,
        event: &MessageDeletedEvent,
    ) -> Result<(), EventPublisherError> {
        let message = ChatEventMessage::MessageDeleted(MessageDeletedMessage::from(event));

        self.producer
            .publish_event(event.channel_id, SCHEMA_CHAT_V1, message)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }
}
