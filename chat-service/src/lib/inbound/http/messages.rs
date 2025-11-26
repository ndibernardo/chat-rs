/// Serializable message types for HTTP layer (infrastructure).
///
/// These types exist to separate domain models from serialization concerns.
/// They handle JSON serialization/deserialization for HTTP requests/responses.
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::domain::channel::errors::ChannelIdError;
use crate::domain::channel::models::ChannelId;
use crate::domain::channel::models::ChannelType;
use crate::domain::message::errors::MessageIdError;
use crate::domain::message::models::MessageId;
use crate::domain::user::errors::UserIdError;
use crate::domain::user::models::UserId;

/// Serializable wrapper for ChannelId.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelIdMessage(pub Uuid);

impl From<ChannelId> for ChannelIdMessage {
    fn from(id: ChannelId) -> Self {
        Self(id.into_uuid())
    }
}

impl From<ChannelIdMessage> for ChannelId {
    fn from(msg: ChannelIdMessage) -> Self {
        Self(msg.0)
    }
}

impl ChannelIdMessage {
    /// Parse from string for HTTP path parameters.
    ///
    /// # Arguments
    /// * `s` - UUID string to parse
    ///
    /// # Returns
    /// Parsed ChannelIdMessage
    ///
    /// # Errors
    /// * `InvalidFormat` - String is not a valid UUID
    pub fn from_string(s: &str) -> Result<Self, ChannelIdError> {
        Uuid::parse_str(s)
            .map(ChannelIdMessage)
            .map_err(|e| ChannelIdError::InvalidFormat(e.to_string()))
    }

    /// Convert to domain ChannelId.
    pub fn into_domain(self) -> ChannelId {
        ChannelId(self.0)
    }
}

/// Serializable wrapper for MessageId.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageIdMessage(pub Uuid);

impl From<MessageId> for MessageIdMessage {
    fn from(id: MessageId) -> Self {
        Self(id.into_uuid())
    }
}

impl From<MessageIdMessage> for MessageId {
    fn from(msg: MessageIdMessage) -> Self {
        Self(msg.0)
    }
}

impl MessageIdMessage {
    /// Parse from string for HTTP path parameters.
    ///
    /// # Arguments
    /// * `s` - UUID string to parse
    ///
    /// # Returns
    /// Parsed MessageIdMessage
    ///
    /// # Errors
    /// * `InvalidFormat` - String is not a valid UUID
    pub fn from_string(s: &str) -> Result<Self, MessageIdError> {
        Uuid::parse_str(s)
            .map(MessageIdMessage)
            .map_err(|e| MessageIdError::InvalidFormat(e.to_string()))
    }

    /// Convert to domain MessageId.
    pub fn into_domain(self) -> MessageId {
        MessageId(self.0)
    }
}

/// Serializable wrapper for UserId.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserIdMessage(pub Uuid);

impl From<UserId> for UserIdMessage {
    fn from(id: UserId) -> Self {
        Self(id.into_uuid())
    }
}

impl From<UserIdMessage> for UserId {
    fn from(msg: UserIdMessage) -> Self {
        Self(msg.0)
    }
}

impl UserIdMessage {
    /// Parse from string for HTTP path parameters.
    ///
    /// # Arguments
    /// * `s` - UUID string to parse
    ///
    /// # Returns
    /// Parsed UserIdMessage
    ///
    /// # Errors
    /// * `InvalidFormat` - String is not a valid UUID
    pub fn from_string(s: &str) -> Result<Self, UserIdError> {
        Uuid::parse_str(s)
            .map(UserIdMessage)
            .map_err(|e| UserIdError::InvalidFormat(e.to_string()))
    }

    /// Convert to domain UserId.
    pub fn into_domain(self) -> UserId {
        UserId(self.0)
    }
}

/// Serializable wrapper for ChannelType.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelTypeMessage {
    Public,
    Private,
    Direct,
}

impl From<ChannelType> for ChannelTypeMessage {
    fn from(channel_type: ChannelType) -> Self {
        match channel_type {
            ChannelType::Public => ChannelTypeMessage::Public,
            ChannelType::Private => ChannelTypeMessage::Private,
            ChannelType::Direct => ChannelTypeMessage::Direct,
        }
    }
}

impl From<ChannelTypeMessage> for ChannelType {
    fn from(msg: ChannelTypeMessage) -> Self {
        match msg {
            ChannelTypeMessage::Public => ChannelType::Public,
            ChannelTypeMessage::Private => ChannelType::Private,
            ChannelTypeMessage::Direct => ChannelType::Direct,
        }
    }
}
