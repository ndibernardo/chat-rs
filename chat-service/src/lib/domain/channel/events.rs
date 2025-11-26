use chrono::DateTime;
use chrono::Utc;
use uuid::Uuid;

use super::models::Channel;
use super::models::ChannelId;
use crate::domain::user::models::UserId;

/// Envelope for all channel-related domain events.
#[derive(Debug, Clone)]
pub enum ChannelEvent {
    ChannelCreated(ChannelCreatedEvent),
    UserJoinedChannel(UserJoinedChannelEvent),
    UserLeftChannel(UserLeftChannelEvent),
    ChannelDeleted(ChannelDeletedEvent),
}

impl ChannelEvent {
    /// Extract the unique event identifier.
    ///
    /// # Returns
    /// Event ID string slice
    pub fn event_id(&self) -> &str {
        match self {
            ChannelEvent::ChannelCreated(e) => &e.event_id,
            ChannelEvent::UserJoinedChannel(e) => &e.event_id,
            ChannelEvent::UserLeftChannel(e) => &e.event_id,
            ChannelEvent::ChannelDeleted(e) => &e.event_id,
        }
    }

    /// Get the event type name.
    ///
    /// # Returns
    /// Event type string ("channel_created", "user_joined_channel", etc.)
    pub fn event_type(&self) -> &str {
        match self {
            ChannelEvent::ChannelCreated(_) => "channel_created",
            ChannelEvent::UserJoinedChannel(_) => "user_joined_channel",
            ChannelEvent::UserLeftChannel(_) => "user_left_channel",
            ChannelEvent::ChannelDeleted(_) => "channel_deleted",
        }
    }

    /// Extract the channel ID this event relates to.
    ///
    /// # Returns
    /// Channel ID reference
    pub fn channel_id(&self) -> ChannelId {
        match self {
            ChannelEvent::ChannelCreated(e) => e.channel_id,
            ChannelEvent::UserJoinedChannel(e) => e.channel_id,
            ChannelEvent::UserLeftChannel(e) => e.channel_id,
            ChannelEvent::ChannelDeleted(e) => e.channel_id,
        }
    }
}

/// Domain event published when a new channel is created.
///
/// Contains snapshot of channel data at creation time for downstream consumers.
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
    /// Create a new ChannelCreated event from a channel entity.
    ///
    /// Generates a unique event ID and extracts channel data.
    ///
    /// # Arguments
    /// * `channel` - Channel entity that was created
    ///
    /// # Returns
    /// ChannelCreatedEvent with unique event ID and channel snapshot
    pub fn new(channel: &Channel) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            channel_id: channel.id(),
            channel_type: channel.channel_type().to_string(),
            name: channel.name().map(|n| n.as_str().to_string()),
            created_by: channel.created_by(),
            timestamp: Utc::now(),
        }
    }
}

/// Domain event published when a user joins a channel.
///
/// For private channels and group membership tracking.
#[derive(Debug, Clone)]
pub struct UserJoinedChannelEvent {
    pub event_id: String,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub timestamp: DateTime<Utc>,
}

impl UserJoinedChannelEvent {
    /// Create a new UserJoinedChannel event.
    ///
    /// Generates a unique event ID and captures current timestamp.
    ///
    /// # Arguments
    /// * `channel_id` - Channel ID that user joined
    /// * `user_id` - User ID who joined
    ///
    /// # Returns
    /// UserJoinedChannelEvent with unique event ID
    pub fn new(channel_id: ChannelId, user_id: UserId) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            channel_id,
            user_id,
            timestamp: Utc::now(),
        }
    }
}

/// Domain event published when a user leaves a channel.
///
/// For private channels and group membership tracking.
#[derive(Debug, Clone)]
pub struct UserLeftChannelEvent {
    pub event_id: String,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub timestamp: DateTime<Utc>,
}

impl UserLeftChannelEvent {
    /// Create a new UserLeftChannel event.
    ///
    /// Generates a unique event ID and captures current timestamp.
    ///
    /// # Arguments
    /// * `channel_id` - Channel ID that user left
    /// * `user_id` - User ID who left
    ///
    /// # Returns
    /// UserLeftChannelEvent with unique event ID
    pub fn new(channel_id: ChannelId, user_id: UserId) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            channel_id,
            user_id,
            timestamp: Utc::now(),
        }
    }
}

/// Domain event published when a channel is deleted.
///
/// Triggers cleanup of associated messages and memberships.
#[derive(Debug, Clone)]
pub struct ChannelDeletedEvent {
    pub event_id: String,
    pub channel_id: ChannelId,
    pub deleted_at: DateTime<Utc>,
}

impl ChannelDeletedEvent {
    /// Create a new ChannelDeleted event.
    ///
    /// Generates a unique event ID and captures current timestamp.
    ///
    /// # Arguments
    /// * `channel_id` - ID of the deleted channel
    ///
    /// # Returns
    /// ChannelDeletedEvent with unique event ID and deletion timestamp
    pub fn new(channel_id: ChannelId) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            channel_id,
            deleted_at: Utc::now(),
        }
    }
}
