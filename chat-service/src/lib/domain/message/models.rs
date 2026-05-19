use std::fmt;

use chrono::DateTime;
use chrono::Utc;
use uuid::Timestamp;
use uuid::Uuid;

use crate::domain::channel::models::ChannelId;
use crate::domain::message::errors::MessageContentError;
use crate::domain::message::errors::MessageIdError;
use crate::domain::user::models::UserId;

/// Message aggregate root entity.
///
/// Represents a single message in a channel with content and metadata.
#[derive(Debug, Clone)]
pub struct Message {
    pub(crate) id: MessageId,
    pub(crate) channel_id: ChannelId,
    pub(crate) user_id: UserId,
    pub(crate) content: MessageContent,
    pub(crate) timestamp: DateTime<Utc>,
}

impl Message {
    /// Create a new message with a generated ID and current timestamp.
    pub fn new(channel_id: ChannelId, user_id: UserId, content: MessageContent) -> Self {
        Self {
            id: MessageId::new_time_based(),
            channel_id,
            user_id,
            content,
            timestamp: Utc::now(),
        }
    }

    /// Get the message ID.
    pub fn id(&self) -> MessageId { self.id }

    /// Get the channel this message belongs to.
    pub fn channel_id(&self) -> ChannelId { self.channel_id }

    /// Get the author user ID.
    pub fn user_id(&self) -> UserId { self.user_id }

    /// Get the message content.
    pub fn content(&self) -> &MessageContent { &self.content }

    /// Get the message send timestamp.
    pub fn timestamp(&self) -> DateTime<Utc> { self.timestamp }
}

/// Message unique identifier value object.
///
/// Uses UUID v1 (TimeUUID) for Cassandra compatibility and time-based ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(Uuid);

impl MessageId {
    /// Generate a new time-based message ID.
    ///
    /// Uses UUID v1 (TimeUUID) which is compatible with Cassandra's timeuuid type
    /// and provides chronological ordering based on timestamp.
    ///
    /// # Returns
    /// MessageId with time-based UUID v1 (TimeUUID)
    pub fn new_time_based() -> Self {
        let timestamp = Timestamp::now(uuid::timestamp::context::NoContext);
        let node_id = [0u8; 6]; // Use a fixed node ID for simplicity
        Self(Uuid::new_v1(timestamp, &node_id))
    }

    /// Parse a message ID from string.
    ///
    /// # Arguments
    /// * `s` - UUID string to parse
    ///
    /// # Returns
    /// Parsed MessageId
    ///
    /// # Errors
    /// * `InvalidFormat` - String is not a valid UUID
    pub fn from_string(s: &str) -> Result<Self, MessageIdError> {
        Uuid::parse_str(s)
            .map(MessageId)
            .map_err(|e| MessageIdError::InvalidFormat(e.to_string()))
    }

    /// Get a reference to the inner UUID.
    ///
    /// # Returns
    /// Reference to the UUID value
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Consume self and return the inner UUID.
    ///
    /// # Returns
    /// The inner UUID value
    pub fn into_uuid(self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for MessageId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Message content value object with validation.
///
/// Ensures content is non-empty and within 4000 character limit.
#[derive(Debug, Clone)]
pub struct MessageContent(String);

impl MessageContent {
    pub(crate) const MAX_LENGTH: usize = 4000;

    /// Create a new validated message content.
    ///
    /// # Arguments
    /// * `content` - Raw message content string
    ///
    /// # Returns
    /// Validated MessageContent value object
    ///
    /// # Errors
    /// * `Empty` - Content is empty string
    /// * `TooLong` - Content exceeds 4000 characters
    pub fn new(content: String) -> Result<Self, MessageContentError> {
        let length = content.len();
        if length == 0 {
            Err(MessageContentError::Empty)
        } else if length > Self::MAX_LENGTH {
            Err(MessageContentError::TooLong {
                max: Self::MAX_LENGTH,
                actual: length,
            })
        } else {
            Ok(Self(content))
        }
    }

    /// Get content as string slice.
    ///
    /// # Returns
    /// Content string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_content_new_returns_content_for_valid_input() {
        let result = MessageContent::new("Has the incident been resolved?".to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "Has the incident been resolved?");
    }

    #[test]
    fn message_content_new_returns_error_for_empty_string() {
        let result = MessageContent::new(String::new());
        assert!(matches!(result, Err(MessageContentError::Empty)));
    }

    #[test]
    fn message_content_new_returns_error_when_content_exceeds_limit() {
        let too_long = "x".repeat(MessageContent::MAX_LENGTH + 1);
        let result = MessageContent::new(too_long);
        assert!(matches!(
            result,
            Err(MessageContentError::TooLong { max: 4000, .. })
        ));
    }

    #[test]
    fn message_content_new_accepts_content_at_exact_limit() {
        let at_limit = "x".repeat(MessageContent::MAX_LENGTH);
        let result = MessageContent::new(at_limit);
        assert!(result.is_ok());
    }
}
