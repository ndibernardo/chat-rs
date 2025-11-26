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
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub content: MessageContent,
    pub timestamp: DateTime<Utc>,
}

/// Message unique identifier value object.
///
/// Uses UUID v1 (TimeUUID) for Cassandra compatibility and time-based ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(pub Uuid);

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
    const MAX_LENGTH: usize = 4000;

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
