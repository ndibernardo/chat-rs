use chrono::DateTime;
use chrono::Utc;

use crate::domain::channel::models::ChannelId;
use crate::domain::message::models::MessageId;
use crate::domain::user::models::UserId;

/// Domain events for the chat service
#[derive(Debug, Clone)]
pub enum ChatEvent {
    MessageSent(MessageSentEvent),
    ChannelCreated(ChannelCreatedEvent),
    UserJoinedChannel(UserJoinedChannelEvent),
    UserLeftChannel(UserLeftChannelEvent),
}

impl ChatEvent {
    pub fn event_id(&self) -> String {
        match self {
            ChatEvent::MessageSent(e) => e.event_id.clone(),
            ChatEvent::ChannelCreated(e) => e.event_id.clone(),
            ChatEvent::UserJoinedChannel(e) => e.event_id.clone(),
            ChatEvent::UserLeftChannel(e) => e.event_id.clone(),
        }
    }

    pub fn event_type(&self) -> &str {
        match self {
            ChatEvent::MessageSent(_) => "message_sent",
            ChatEvent::ChannelCreated(_) => "channel_created",
            ChatEvent::UserJoinedChannel(_) => "user_joined_channel",
            ChatEvent::UserLeftChannel(_) => "user_left_channel",
        }
    }
}

/// Event emitted when a new message is sent
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
    pub fn new(
        message_id: MessageId,
        channel_id: ChannelId,
        user_id: UserId,
        content: String,
    ) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            message_id,
            channel_id,
            user_id,
            content,
            timestamp: Utc::now(),
        }
    }
}

/// Event emitted when a new channel is created
#[derive(Debug, Clone)]
pub struct ChannelCreatedEvent {
    pub event_id: String,
    pub channel_id: ChannelId,
    pub channel_type: String,
    pub name: Option<String>,
    pub created_by: UserId,
    pub timestamp: DateTime<Utc>,
}

impl ChannelCreatedEvent {
    pub fn new(
        channel_id: ChannelId,
        channel_type: String,
        name: Option<String>,
        created_by: UserId,
    ) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            channel_id,
            channel_type,
            name,
            created_by,
            timestamp: Utc::now(),
        }
    }
}

/// Event emitted when a user joins a channel
#[derive(Debug, Clone)]
pub struct UserJoinedChannelEvent {
    pub event_id: String,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub timestamp: DateTime<Utc>,
}

impl UserJoinedChannelEvent {
    pub fn new(channel_id: ChannelId, user_id: UserId) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            channel_id,
            user_id,
            timestamp: Utc::now(),
        }
    }
}

/// Event emitted when a user leaves a channel
#[derive(Debug, Clone)]
pub struct UserLeftChannelEvent {
    pub event_id: String,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub timestamp: DateTime<Utc>,
}

impl UserLeftChannelEvent {
    pub fn new(channel_id: ChannelId, user_id: UserId) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            channel_id,
            user_id,
            timestamp: Utc::now(),
        }
    }
}
