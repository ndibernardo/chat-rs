use async_trait::async_trait;

use super::events::UserCreatedEvent;
use super::events::UserDeletedEvent;
use super::events::UserUpdatedEvent;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;

/// Port for user-service communication (via gRPC).
#[async_trait]
pub trait UserServicePort: Send + Sync + 'static {
    /// Get user by ID from user-service.
    ///
    /// # Arguments
    /// * `user_id` - User ID to retrieve
    ///
    /// # Returns
    /// User if found, None if not found
    ///
    /// # Errors
    /// Returns error string if gRPC call fails
    async fn get_user(&self, user_id: UserId) -> Result<Option<User>, String>;
}

/// Port for local user replica repository.
///
/// Maintains a denormalized copy of user data from user-service events.
/// Updated via UserEventConsumer when user events arrive from Kafka.
#[async_trait]
pub trait UserReplicaRepository: Send + Sync + 'static {
    /// Upsert user in replica (insert or update).
    ///
    /// # Arguments
    /// * `user` - User data to store
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// Returns error string if database operation fails
    async fn upsert(&self, user: User) -> Result<(), String>;

    /// Delete user from replica.
    ///
    /// # Arguments
    /// * `user_id` - User ID to delete
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// Returns error string if database operation fails
    async fn delete(&self, user_id: UserId) -> Result<(), String>;

    /// Get user from replica by ID.
    ///
    /// # Arguments
    /// * `user_id` - User ID to retrieve
    ///
    /// # Returns
    /// User if found, None if not found
    ///
    /// # Errors
    /// Returns error string if database operation fails
    async fn get(&self, user_id: UserId) -> Result<Option<User>, String>;

    /// Get multiple users from replica by IDs.
    ///
    /// # Arguments
    /// * `user_ids` - Slice of user IDs to retrieve
    ///
    /// # Returns
    /// Vector of found users (missing IDs are skipped without error)
    ///
    /// # Errors
    /// Returns error string if database operation fails
    async fn get_many(&self, user_ids: &[UserId]) -> Result<Vec<User>, String>;
}

/// Event consumer for user-service domain events.
///
/// Processes events from user-service to maintain eventual consistency in chat-service.
/// Handles local read model updates and cleanup operations.
#[async_trait]
pub trait UserEventConsumer: Send + Sync + 'static {
    /// Handle user creation event.
    ///
    /// Updates local user replica for read-path enrichment.
    ///
    /// # Arguments
    /// * `event` - UserCreated event from user-service
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * Database operation failed
    /// * Invalid user data in event
    async fn handle_user_created(&self, event: &UserCreatedEvent) -> Result<(), String>;

    /// Handle user update event.
    ///
    /// Updates local user replica with latest user data.
    ///
    /// # Arguments
    /// * `event` - UserUpdated event from user-service
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * Database operation failed
    /// * Invalid user data in event
    async fn handle_user_updated(&self, event: &UserUpdatedEvent) -> Result<(), String>;

    /// Handle user deletion event.
    ///
    /// Performs cleanup of orphaned data:
    /// - Deletes channels created by user
    /// - Deletes messages sent by user
    /// - Removes user from local replica
    ///
    /// # Arguments
    /// * `event` - UserDeleted event from user-service
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * Database operation failed during cleanup
    /// * Invalid user ID in event
    async fn handle_user_deleted(&self, event: &UserDeletedEvent) -> Result<(), String>;
}
