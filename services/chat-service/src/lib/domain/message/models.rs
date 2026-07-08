use std::fmt;
use std::sync::LazyLock;

use chrono::DateTime;
use chrono::Utc;
use uuid::timestamp::context::Context;
use uuid::Timestamp;
use uuid::Uuid;

use crate::domain::channel::models::ChannelId;
use crate::domain::message::errors::MessageContentError;
use crate::domain::message::errors::MessageIdError;
use crate::domain::message::errors::MessageLimitError;
use crate::domain::user::models::UserId;

/// Message aggregate root entity.
///
/// Represents a single message in a channel with content and metadata.
#[derive(Debug, Clone)]
pub struct Message {
    id: MessageId,
    channel_id: ChannelId,
    user_id: UserId,
    content: MessageContent,
    timestamp: DateTime<Utc>,
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

    /// Reconstruct a message from persisted parts — for use within the crate
    /// only (e.g. database mapping).
    pub(crate) fn from_parts(
        id: MessageId,
        channel_id: ChannelId,
        user_id: UserId,
        content: MessageContent,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            channel_id,
            user_id,
            content,
            timestamp,
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

/// Per-process node id for v1 generation: random rather than a real MAC
/// address, with the multicast bit set per RFC 9562 §5.1 to mark it as such.
/// Unlike the fixed `[0u8; 6]` this used to be, every instance now gets its
/// own value, so two instances can no longer produce colliding IDs for
/// messages sent in the same 100ns tick.
static NODE_ID: LazyLock<[u8; 6]> = LazyLock::new(|| {
    let mut node_id: [u8; 6] = Uuid::new_v4().as_bytes()[..6].try_into().unwrap();
    node_id[0] |= 0x01;
    node_id
});

/// Per-process clock sequence, seeded with a random starting value (instead
/// of `NoContext`, which always starts — and stays — at 0) so that two
/// messages generated within the same 100ns tick on this instance get
/// distinct sequence numbers.
static CLOCK_CONTEXT: LazyLock<Context> = LazyLock::new(Context::new_random);

impl MessageId {
    /// Generate a new time-based message ID.
    ///
    /// Uses UUID v1 (TimeUUID): required by Cassandra/ScyllaDB's `timeuuid`
    /// column type, which does not accept other UUID versions. Collision
    /// risk is addressed by using a random per-process node id and a
    /// randomly-seeded clock sequence instead of a fixed node id shared by
    /// every instance and an always-zero clock sequence.
    ///
    /// # Returns
    /// MessageId with time-based UUID v1 (TimeUUID)
    pub fn new_time_based() -> Self {
        let timestamp = Timestamp::now(&*CLOCK_CONTEXT);
        Self(Uuid::new_v1(timestamp, &NODE_ID))
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

    /// Construct from a raw UUID — for use within the crate only (e.g. database mapping).
    pub(crate) fn from_uuid(uuid: Uuid) -> Self {
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

/// Bounded pagination limit for message queries.
///
/// Guards against negative or unbounded values flowing from a query string
/// straight into the database layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit(i32);

impl Limit {
    pub const MIN: i32 = 1;
    pub const MAX: i32 = 100;
    pub const DEFAULT: i32 = 50;

    /// Validate a requested limit.
    ///
    /// # Errors
    /// * `OutOfRange` - value is outside `[MIN, MAX]`
    pub fn new(value: i32) -> Result<Self, MessageLimitError> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(MessageLimitError::OutOfRange {
                min: Self::MIN,
                max: Self::MAX,
                actual: value,
            })
        }
    }

    /// Get the validated limit as a raw integer, for binding to a query parameter.
    pub fn value(&self) -> i32 {
        self.0
    }
}

impl Default for Limit {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_time_based_does_not_collide_within_the_same_tick() {
        let ids: std::collections::HashSet<MessageId> =
            (0..10_000).map(|_| MessageId::new_time_based()).collect();
        assert_eq!(ids.len(), 10_000);
    }

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

    #[test]
    fn limit_new_accepts_value_within_bounds() {
        let result = Limit::new(10);
        assert_eq!(result.unwrap().value(), 10);
    }

    #[test]
    fn limit_new_returns_error_for_zero_or_negative() {
        assert!(matches!(
            Limit::new(0),
            Err(MessageLimitError::OutOfRange { .. })
        ));
        assert!(matches!(
            Limit::new(-1),
            Err(MessageLimitError::OutOfRange { .. })
        ));
    }

    #[test]
    fn limit_new_returns_error_above_max() {
        let result = Limit::new(Limit::MAX + 1);
        assert!(matches!(result, Err(MessageLimitError::OutOfRange { .. })));
    }

    #[test]
    fn limit_default_is_within_bounds() {
        let limit = Limit::default();
        assert_eq!(limit.value(), Limit::DEFAULT);
    }
}
