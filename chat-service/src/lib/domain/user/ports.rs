use async_trait::async_trait;

use crate::domain::user::errors::UserError;
use crate::domain::user::models::ResolvedUser;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;

/// Resolves user information, using the local replica first and falling back to user-service.
///
/// Implementations should check the local read-model before making a remote call.
#[async_trait]
pub trait UserResolver: Send + Sync + 'static {
    /// Look up a user by ID, checking the local replica before calling user-service.
    ///
    /// # Arguments
    /// * `user_id` - User ID to resolve
    ///
    /// # Returns
    /// User if found in either source, `None` if not found anywhere
    ///
    /// # Errors
    /// Returns `UserError` if both sources fail
    async fn resolve(&self, user_id: UserId) -> Result<Option<ResolvedUser>, UserError>;
}

/// Port for looking up a user from the remote user-service.
#[async_trait]
pub trait RemoteUserLookup: Send + Sync + 'static {
    /// Get user by ID from user-service.
    ///
    /// # Arguments
    /// * `user_id` - User ID to retrieve
    ///
    /// # Returns
    /// User if found, None if not found
    ///
    /// # Errors
    /// Returns `UserError` if the remote call fails
    async fn get_user(&self, user_id: UserId) -> Result<Option<ResolvedUser>, UserError>;
}

/// Port for local user replica repository.
///
/// Maintains a denormalized copy of user data from user-service events.
/// Updated via UserEventConsumer when user events arrive.
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
    /// Returns `UserError` if the database operation fails
    async fn upsert(&self, user: User) -> Result<(), UserError>;

    /// Delete user from replica.
    ///
    /// # Arguments
    /// * `user_id` - User ID to delete
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// Returns `UserError` if the database operation fails
    async fn delete(&self, user_id: UserId) -> Result<(), UserError>;

    /// Get user from replica by ID.
    ///
    /// # Arguments
    /// * `user_id` - User ID to retrieve
    ///
    /// # Returns
    /// User if found, None if not found
    ///
    /// # Errors
    /// Returns `UserError` if the database operation fails
    async fn get(&self, user_id: UserId) -> Result<Option<User>, UserError>;

    /// Get multiple users from replica by IDs.
    ///
    /// # Arguments
    /// * `user_ids` - Slice of user IDs to retrieve
    ///
    /// # Returns
    /// Vector of found users (missing IDs are skipped without error)
    ///
    /// # Errors
    /// Returns `UserError` if the database operation fails
    async fn get_many(&self, user_ids: &[UserId]) -> Result<Vec<User>, UserError>;
}
