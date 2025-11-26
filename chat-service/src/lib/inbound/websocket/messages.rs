/// WebSocket message types for client-server communication.
///
/// These types handle JSON serialization/deserialization for WebSocket messages.
/// Uses type-safe wrappers around domain types while maintaining clean JSON serialization.
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::domain::channel::models::ChannelId;
use crate::domain::message::models::MessageId;
use crate::domain::user::models::UserId;

/// Serializable wrapper for MessageId in WebSocket messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WsMessageId(Uuid);

impl From<MessageId> for WsMessageId {
    fn from(id: MessageId) -> Self {
        Self(id.into_uuid())
    }
}

impl From<WsMessageId> for MessageId {
    fn from(id: WsMessageId) -> Self {
        Self(id.0)
    }
}

/// Serializable wrapper for UserId in WebSocket messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WsUserId(Uuid);

impl From<UserId> for WsUserId {
    fn from(id: UserId) -> Self {
        Self(id.into_uuid())
    }
}

impl From<WsUserId> for UserId {
    fn from(id: WsUserId) -> Self {
        Self(id.0)
    }
}

/// Serializable wrapper for ChannelId in WebSocket messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WsChannelId(Uuid);

impl From<ChannelId> for WsChannelId {
    fn from(id: ChannelId) -> Self {
        Self(id.into_uuid())
    }
}

impl From<WsChannelId> for ChannelId {
    fn from(id: WsChannelId) -> Self {
        Self(id.0)
    }
}

/// WebSocket message types from client.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Send a message to the channel.
    SendMessage { content: String },
    /// Ping to keep connection alive.
    Ping,
}

/// WebSocket message types sent to client.
///
/// Uses type-safe wrappers that serialize transparently to UUID strings.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// New message received in the channel.
    NewMessage {
        id: WsMessageId,
        user_id: WsUserId,
        content: String,
        timestamp: DateTime<Utc>,
    },
    /// Error message.
    Error { message: String },
    /// Pong response to ping.
    Pong,
    /// Connection established confirmation.
    Connected { channel_id: WsChannelId },
}
