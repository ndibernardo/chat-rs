/// Infrastructure message types for event serialization.
///
/// These types are used in the infrastructure layer for Kafka event publishing/consuming.
/// They are separate from pure domain events to maintain domain layer purity.
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use crate::domain::channel::events::ChannelCreatedEvent;
use crate::domain::channel::events::ChannelDeletedEvent;
use crate::domain::channel::events::UserJoinedChannelEvent;
use crate::domain::channel::events::UserLeftChannelEvent;
use crate::domain::message::events::MessageDeletedEvent;
use crate::domain::message::events::MessageSentEvent;
use crate::domain::user::events::UserCreatedEvent;
use crate::domain::user::events::UserDeletedEvent;
use crate::domain::user::events::UserEvent;
use crate::domain::user::events::UserUpdatedEvent;

/// Serializable envelope for all chat-service events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum ChatEventMessage {
    MessageSent(MessageSentMessage),
    ChannelCreated(ChannelCreatedMessage),
    UserJoinedChannel(UserJoinedChannelMessage),
    UserLeftChannel(UserLeftChannelMessage),
}

impl ChatEventMessage {
    pub fn event_id(&self) -> &str {
        match self {
            ChatEventMessage::MessageSent(e) => &e.event_id,
            ChatEventMessage::ChannelCreated(e) => &e.event_id,
            ChatEventMessage::UserJoinedChannel(e) => &e.event_id,
            ChatEventMessage::UserLeftChannel(e) => &e.event_id,
        }
    }

    pub fn event_type(&self) -> &str {
        match self {
            ChatEventMessage::MessageSent(_) => "message_sent",
            ChatEventMessage::ChannelCreated(_) => "channel_created",
            ChatEventMessage::UserJoinedChannel(_) => "user_joined_channel",
            ChatEventMessage::UserLeftChannel(_) => "user_left_channel",
        }
    }
}

/// Serializable message for MessageSent event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSentMessage {
    pub event_id: String,
    pub message_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl From<&MessageSentEvent> for MessageSentMessage {
    fn from(event: &MessageSentEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            message_id: event.message_id.to_string(),
            channel_id: event.channel_id.to_string(),
            user_id: event.user_id.to_string(),
            content: event.content.clone(),
            timestamp: event.timestamp,
        }
    }
}

/// Serializable message for MessageDeleted event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeletedMessage {
    pub event_id: String,
    pub message_id: String,
    pub channel_id: String,
    pub deleted_at: DateTime<Utc>,
}

impl From<&MessageDeletedEvent> for MessageDeletedMessage {
    fn from(event: &MessageDeletedEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            message_id: event.message_id.to_string(),
            channel_id: event.channel_id.to_string(),
            deleted_at: event.deleted_at,
        }
    }
}

/// Serializable message for ChannelCreated event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCreatedMessage {
    pub event_id: String,
    pub channel_id: String,
    pub channel_type: String,
    pub name: Option<String>,
    pub created_by: String,
    pub timestamp: DateTime<Utc>,
}

impl From<&ChannelCreatedEvent> for ChannelCreatedMessage {
    fn from(event: &ChannelCreatedEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            channel_id: event.channel_id.to_string(),
            channel_type: event.channel_type.clone(),
            name: event.name.clone(),
            created_by: event.created_by.to_string(),
            timestamp: event.timestamp,
        }
    }
}

/// Serializable message for ChannelDeleted event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelDeletedMessage {
    pub event_id: String,
    pub channel_id: String,
    pub deleted_at: DateTime<Utc>,
}

impl From<&ChannelDeletedEvent> for ChannelDeletedMessage {
    fn from(event: &ChannelDeletedEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            channel_id: event.channel_id.to_string(),
            deleted_at: event.deleted_at,
        }
    }
}

/// Serializable message for UserJoinedChannel event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserJoinedChannelMessage {
    pub event_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub timestamp: DateTime<Utc>,
}

impl From<&UserJoinedChannelEvent> for UserJoinedChannelMessage {
    fn from(event: &UserJoinedChannelEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            channel_id: event.channel_id.to_string(),
            user_id: event.user_id.to_string(),
            timestamp: event.timestamp,
        }
    }
}

/// Serializable message for UserLeftChannel event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserLeftChannelMessage {
    pub event_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub timestamp: DateTime<Utc>,
}

impl From<&UserLeftChannelEvent> for UserLeftChannelMessage {
    fn from(event: &UserLeftChannelEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            channel_id: event.channel_id.to_string(),
            user_id: event.user_id.to_string(),
            timestamp: event.timestamp,
        }
    }
}

/// Serializable envelope for user-service events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum UserEventMessage {
    UserCreated(UserCreatedMessage),
    UserUpdated(UserUpdatedMessage),
    UserDeleted(UserDeletedMessage),
}

impl TryFrom<UserEventMessage> for UserEvent {
    type Error = String;

    fn try_from(message: UserEventMessage) -> Result<Self, Self::Error> {
        match message {
            UserEventMessage::UserCreated(m) => Ok(UserEvent::UserCreated(UserCreatedEvent {
                event_id: m.event_id,
                user_id: m.user_id,
                username: m.username,
                email: m.email,
                created_at: m.created_at,
            })),
            UserEventMessage::UserUpdated(m) => Ok(UserEvent::UserUpdated(UserUpdatedEvent {
                event_id: m.event_id,
                user_id: m.user_id,
                username: m.username,
                email: m.email,
                updated_at: m.updated_at,
            })),
            UserEventMessage::UserDeleted(m) => Ok(UserEvent::UserDeleted(UserDeletedEvent {
                event_id: m.event_id,
                user_id: m.user_id,
                deleted_at: m.deleted_at,
            })),
        }
    }
}

/// Serializable message for UserCreated event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCreatedMessage {
    pub event_id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

/// Serializable message for UserUpdated event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUpdatedMessage {
    pub event_id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub updated_at: DateTime<Utc>,
}

/// Serializable message for UserDeleted event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDeletedMessage {
    pub event_id: String,
    pub user_id: String,
    pub deleted_at: DateTime<Utc>,
}
