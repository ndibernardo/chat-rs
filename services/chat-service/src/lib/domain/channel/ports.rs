use async_trait::async_trait;

use super::events::ChannelCreatedEvent;
use super::models::Channel;
use super::models::ChannelId;
use super::models::CreateChannelCommand;
use crate::domain::channel::errors::ChannelError;
use crate::domain::user::models::UserId;

/// Port for channel domain service operations.
#[async_trait]
pub trait ChannelService: Send + Sync + 'static {
    /// Create a new channel of specified type.
    ///
    /// # Arguments
    /// * `command` - Create channel command (Public, Private, or Direct)
    /// * `created_by` - User creating the channel
    ///
    /// # Returns
    /// Created channel entity
    ///
    /// # Errors
    /// * `NameAlreadyExists` - Channel name already taken (public/private only)
    /// * `DatabaseError` - Database operation failed
    async fn create_channel(
        &self,
        command: CreateChannelCommand,
        created_by: UserId,
    ) -> Result<Channel, ChannelError>;

    /// Retrieve channel by unique identifier.
    ///
    /// # Arguments
    /// * `id` - Channel ID to find
    ///
    /// # Returns
    /// Channel entity
    ///
    /// # Errors
    /// * `NotFound` - Channel does not exist
    /// * `DatabaseError` - Database operation failed
    async fn get_channel(&self, id: ChannelId) -> Result<Channel, ChannelError>;

    /// List all publicly accessible channels.
    ///
    /// # Returns
    /// Vector of all public channels
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn list_public_channels(&self) -> Result<Vec<Channel>, ChannelError>;

    /// List channels accessible to a specific user.
    ///
    /// Includes channels created by user, private channels where user is member,
    /// and direct channels where user is participant.
    ///
    /// # Arguments
    /// * `user_id` - User ID to search for
    ///
    /// # Returns
    /// Vector of accessible channels
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn list_user_channels(&self, user_id: UserId) -> Result<Vec<Channel>, ChannelError>;
}

/// Repository port for channel persistence operations.
#[async_trait]
pub trait ChannelRepository: Send + Sync + 'static {
    /// Persist a new channel entity and enqueue its outbox event in the
    /// same transaction.
    ///
    /// # Arguments
    /// * `channel` - Channel entity to create
    /// * `event` - ChannelCreated event to enqueue for the outbox relay
    ///
    /// # Returns
    /// Created channel with database-assigned metadata
    ///
    /// # Errors
    /// * `NameAlreadyExists` - Channel name already taken
    /// * `DatabaseError` - Database operation failed
    async fn create(
        &self,
        channel: Channel,
        event: &ChannelCreatedEvent,
    ) -> Result<Channel, ChannelError>;

    /// Retrieve channel by unique identifier.
    ///
    /// # Arguments
    /// * `id` - Channel ID to find
    ///
    /// # Returns
    /// Channel entity if found, None otherwise
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn find_by_id(&self, id: ChannelId) -> Result<Option<Channel>, ChannelError>;

    /// List all public channels.
    ///
    /// # Returns
    /// Vector of all public channels
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn find_public_channels(&self) -> Result<Vec<Channel>, ChannelError>;

    /// Find channels accessible to a specific user.
    ///
    /// Includes channels created by user, private channels where user is member,
    /// and direct channels where user is participant.
    ///
    /// # Arguments
    /// * `user_id` - User ID to search for
    ///
    /// # Returns
    /// Vector of accessible channels
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn find_by_user(&self, user_id: UserId) -> Result<Vec<Channel>, ChannelError>;

    /// Remove channel permanently.
    ///
    /// # Arguments
    /// * `id` - Channel ID to delete
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * `NotFound` - Channel does not exist
    /// * `DatabaseError` - Database operation failed
    async fn delete(&self, id: ChannelId) -> Result<(), ChannelError>;
}
