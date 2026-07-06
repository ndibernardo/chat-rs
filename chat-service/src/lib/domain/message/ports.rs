use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;

use super::events::MessageDeletedEvent;
use super::events::MessageSentEvent;
use super::models::Message;
use super::models::MessageContent;
use crate::domain::channel::models::ChannelId;
use crate::domain::channel::models::Membership;
use crate::domain::errors::EventPublisherError;
use crate::domain::message::errors::MessageError;
use crate::domain::user::models::UserId;

/// Port for message domain service operations.
#[async_trait]
pub trait MessageService: Send + Sync + 'static {
    /// Send a message to a channel.
    ///
    /// Publishes MessageSentEvent to Kafka if event producer is configured.
    /// Broadcasts to WebSocket clients if broadcaster is configured.
    ///
    /// # Arguments
    /// * `membership` - Proof the sender was verified as a member of the target channel
    /// * `content` - Validated message content
    ///
    /// # Returns
    /// Created message entity
    ///
    /// # Errors
    /// * `UserNotFound` - Sender no longer exists
    /// * `DatabaseError` - Database operation failed
    async fn send_message(
        &self,
        membership: Membership,
        content: MessageContent,
    ) -> Result<Message, MessageError>;

    /// Retrieve messages from a channel with pagination.
    ///
    /// Returns messages in reverse chronological order (newest first).
    ///
    /// # Arguments
    /// * `membership` - Proof the caller was verified as a member of the channel
    /// * `limit` - Maximum number of messages to return
    /// * `before` - Optional timestamp cursor for pagination (fetch messages before this time)
    ///
    /// # Returns
    /// Vector of messages ordered by timestamp descending
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn get_channel_messages(
        &self,
        membership: Membership,
        limit: i32,
        before: Option<DateTime<Utc>>,
    ) -> Result<Vec<Message>, MessageError>;
}

/// Repository port for message persistence operations.
///
/// Optimized for time-series data storage.
#[async_trait]
pub trait MessageRepository: Send + Sync + 'static {
    /// Persist a new message entity.
    ///
    /// # Arguments
    /// * `message` - Message entity to create
    ///
    /// # Returns
    /// Created message with database-assigned metadata
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn create(&self, message: Message) -> Result<Message, MessageError>;

    /// Retrieve messages from channel with pagination.
    ///
    /// Returns messages in reverse chronological order (newest first).
    ///
    /// # Arguments
    /// * `channel_id` - Channel ID to query
    /// * `limit` - Maximum number of messages to return
    /// * `before` - Optional timestamp cursor for pagination (fetch messages before this time)
    ///
    /// # Returns
    /// Vector of messages ordered by timestamp descending
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn find_by_channel(
        &self,
        channel_id: ChannelId,
        limit: i32,
        before: Option<DateTime<Utc>>,
    ) -> Result<Vec<Message>, MessageError>;

    /// Retrieve messages sent by a specific user.
    ///
    /// Returns messages in reverse chronological order (newest first).
    ///
    /// # Arguments
    /// * `user_id` - User ID to search for
    /// * `limit` - Maximum number of messages to return
    ///
    /// # Returns
    /// Vector of messages ordered by timestamp descending
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn find_by_user(&self, user_id: UserId, limit: i32)
        -> Result<Vec<Message>, MessageError>;
}

/// Event publishing for message domain events.
#[async_trait]
pub trait MessageEventPublisher: Send + Sync + 'static {
    /// Publish message sent event.
    ///
    /// # Arguments
    /// * `event` - MessageSent event
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * `SerializationFailed` - Event serialization failed
    /// * `PublishFailed` - Failed to publish to broker
    /// * `ConnectionFailed` - Broker connection failed
    /// * `Timeout` - Publishing timed out
    async fn publish_message_sent(
        &self,
        event: &MessageSentEvent,
    ) -> Result<(), EventPublisherError>;

    /// Publish message deletion event.
    ///
    /// # Arguments
    /// * `event` - MessageDeleted event
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * `SerializationFailed` - Event serialization failed
    /// * `PublishFailed` - Failed to publish to broker
    /// * `ConnectionFailed` - Broker connection failed
    /// * `Timeout` - Publishing timed out
    async fn publish_message_deleted(
        &self,
        event: &MessageDeletedEvent,
    ) -> Result<(), EventPublisherError>;
}

/// Port for broadcasting a sent message to clients connected to its channel.
///
/// Implemented by the delivery-mechanism adapter (the WebSocket connection
/// registry). Inbound Kafka consumers depend on this domain port instead of
/// the concrete registry, so event consumption stays decoupled from the
/// delivery transport.
#[async_trait]
pub trait MessageBroadcaster: Send + Sync + 'static {
    /// Broadcast a message to clients connected to its channel on this instance.
    async fn broadcast(&self, message: &Message);
}
