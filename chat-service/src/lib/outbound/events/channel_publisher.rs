use std::sync::Arc;

use async_trait::async_trait;

use super::messages::ChannelCreatedMessage;
use super::messages::ChannelDeletedMessage;
use super::messages::ChatEventMessage;
use super::messages::UserJoinedChannelMessage;
use super::messages::UserLeftChannelMessage;
use super::producer::KafkaEventProducer;
use crate::domain::channel::events::ChannelCreatedEvent;
use crate::domain::channel::events::ChannelDeletedEvent;
use crate::domain::channel::events::UserJoinedChannelEvent;
use crate::domain::channel::events::UserLeftChannelEvent;
use crate::domain::channel::ports::ChannelEventPublisher;
use crate::domain::errors::EventPublisherError;

/// Kafka implementation of ChannelEventPublisher.
pub struct KafkaChannelEventPublisher {
    producer: Arc<KafkaEventProducer>,
}

impl KafkaChannelEventPublisher {
    /// Create a new Kafka channel event publisher.
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
impl ChannelEventPublisher for KafkaChannelEventPublisher {
    async fn publish_channel_created(
        &self,
        event: &ChannelCreatedEvent,
    ) -> Result<(), EventPublisherError> {
        let envelope = ChatEventMessage::ChannelCreated(ChannelCreatedMessage::from(event));
        self.producer
            .publish_event(event.channel_id, &event.event_id, &envelope)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }

    async fn publish_user_joined_channel(
        &self,
        event: &UserJoinedChannelEvent,
    ) -> Result<(), EventPublisherError> {
        let envelope =
            ChatEventMessage::UserJoinedChannel(UserJoinedChannelMessage::from(event));
        self.producer
            .publish_event(event.channel_id, &event.event_id, &envelope)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }

    async fn publish_user_left_channel(
        &self,
        event: &UserLeftChannelEvent,
    ) -> Result<(), EventPublisherError> {
        let envelope = ChatEventMessage::UserLeftChannel(UserLeftChannelMessage::from(event));
        self.producer
            .publish_event(event.channel_id, &event.event_id, &envelope)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }

    async fn publish_channel_deleted(
        &self,
        event: &ChannelDeletedEvent,
    ) -> Result<(), EventPublisherError> {
        let envelope = ChatEventMessage::ChannelDeleted(ChannelDeletedMessage::from(event));
        self.producer
            .publish_event(event.channel_id, &event.event_id, &envelope)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }
}
