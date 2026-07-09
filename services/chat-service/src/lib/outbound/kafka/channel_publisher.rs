use std::sync::Arc;

use async_trait::async_trait;

use super::envelope::SCHEMA_CHAT_V1;
use super::messages::ChannelCreatedMessage;
use super::messages::ChannelDeletedMessage;
use super::messages::ChatEventMessage;
use super::messages::UserJoinedChannelMessage;
use super::messages::UserLeftChannelMessage;
use super::producer::EventProducer;
use crate::domain::channel::events::ChannelCreatedEvent;
use crate::domain::channel::events::ChannelDeletedEvent;
use crate::domain::channel::events::UserJoinedChannelEvent;
use crate::domain::channel::events::UserLeftChannelEvent;
use crate::domain::channel::ports;
use crate::domain::errors::EventPublisherError;

/// Kafka implementation of ChannelEventPublisher.
pub struct ChannelEventPublisher {
    producer: Arc<EventProducer>,
}

impl ChannelEventPublisher {
    /// Create a new Kafka channel event publisher.
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
impl ports::ChannelEventPublisher for ChannelEventPublisher {
    async fn publish_channel_created(
        &self,
        event: &ChannelCreatedEvent,
    ) -> Result<(), EventPublisherError> {
        let message = ChatEventMessage::ChannelCreated(ChannelCreatedMessage::from(event));
        self.producer
            .publish_event(event.channel_id, SCHEMA_CHAT_V1, message)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }

    async fn publish_user_joined_channel(
        &self,
        event: &UserJoinedChannelEvent,
    ) -> Result<(), EventPublisherError> {
        let message = ChatEventMessage::UserJoinedChannel(UserJoinedChannelMessage::from(event));
        self.producer
            .publish_event(event.channel_id, SCHEMA_CHAT_V1, message)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }

    async fn publish_user_left_channel(
        &self,
        event: &UserLeftChannelEvent,
    ) -> Result<(), EventPublisherError> {
        let message = ChatEventMessage::UserLeftChannel(UserLeftChannelMessage::from(event));
        self.producer
            .publish_event(event.channel_id, SCHEMA_CHAT_V1, message)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }

    async fn publish_channel_deleted(
        &self,
        event: &ChannelDeletedEvent,
    ) -> Result<(), EventPublisherError> {
        let message = ChatEventMessage::ChannelDeleted(ChannelDeletedMessage::from(event));
        self.producer
            .publish_event(event.channel_id, SCHEMA_CHAT_V1, message)
            .await
            .map_err(|e| EventPublisherError::PublishFailed(e.to_string()))
    }
}
