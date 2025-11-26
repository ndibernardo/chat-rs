use chrono::DateTime;
use chrono::Utc;
use uuid::Uuid;

use super::models::Message;
use super::models::MessageId;
use crate::domain::channel::models::ChannelId;
use crate::domain::user::models::UserId;

/// Envelope for all message-related domain events.
#[derive(Debug, Clone)]
pub enum MessageEvent {
    MessageSent(MessageSentEvent),
    MessageDeleted(MessageDeletedEvent),
}

impl MessageEvent {
    /// Extract the unique event identifier.
    ///
    /// # Returns
    /// Event ID string slice
    pub fn event_id(&self) -> &str {
        match self {
            MessageEvent::MessageSent(e) => &e.event_id,
            MessageEvent::MessageDeleted(e) => &e.event_id,
        }
    }

    /// Get the event type name.
    ///
    /// # Returns
    /// Event type string ("message_sent" or "message_deleted")
    pub fn event_type(&self) -> &str {
        match self {
            MessageEvent::MessageSent(_) => "message_sent",
            MessageEvent::MessageDeleted(_) => "message_deleted",
        }
    }

    /// Extract the message ID this event relates to.
    ///
    /// # Returns
    /// Message ID reference
    pub fn message_id(&self) -> MessageId {
        match self {
            MessageEvent::MessageSent(e) => e.message_id,
            MessageEvent::MessageDeleted(e) => e.message_id,
        }
    }
}

/// Domain event published when a new message is sent.
///
/// Contains snapshot of message data for downstream consumers (WebSocket broadcast, notifications, etc.).
#[derive(Debug, Clone)]
pub struct MessageSentEvent {
    pub event_id: String,
    pub message_id: MessageId,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl MessageSentEvent {
    /// Create a new MessageSent event from a message entity.
    ///
    /// Generates a unique event ID and extracts message data.
    ///
    /// # Arguments
    /// * `message` - Message entity that was sent
    ///
    /// # Returns
    /// MessageSentEvent with unique event ID and message snapshot
    pub fn new(message: &Message) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            message_id: message.id,
            channel_id: message.channel_id,
            user_id: message.user_id,
            content: message.content.as_str().to_string(),
            timestamp: message.timestamp,
        }
    }
}

/// Domain event published when a message is deleted.
///
/// Contains only message ID and deletion timestamp for cleanup operations.
#[derive(Debug, Clone)]
pub struct MessageDeletedEvent {
    pub event_id: String,
    pub message_id: MessageId,
    pub channel_id: ChannelId,
    pub deleted_at: DateTime<Utc>,
}

impl MessageDeletedEvent {
    /// Create a new MessageDeleted event.
    ///
    /// Generates a unique event ID and captures current timestamp.
    ///
    /// # Arguments
    /// * `message_id` - ID of the deleted message
    /// * `channel_id` - ID of the channel containing the message
    ///
    /// # Returns
    /// MessageDeletedEvent with unique event ID and deletion timestamp
    pub fn new(message_id: MessageId, channel_id: ChannelId) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            message_id,
            channel_id,
            deleted_at: Utc::now(),
        }
    }
}
