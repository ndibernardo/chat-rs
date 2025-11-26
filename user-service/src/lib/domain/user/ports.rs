use async_trait::async_trait;

use crate::domain::user::events::UserCreatedEvent;
use crate::domain::user::events::UserDeletedEvent;
use crate::domain::user::events::UserUpdatedEvent;
use crate::domain::user::models::CreateUserCommand;
use crate::domain::user::models::UpdateUserCommand;
use crate::domain::user::models::User;
use crate::domain::user::models::UserId;
use crate::user::errors::EventPublisherError;
use crate::user::errors::UserError;
use crate::user::models::Username;

/// Port for user domain service operations.
#[async_trait]
pub trait UserServicePort: Send + Sync + 'static {
    /// Create new user with validated credentials.
    ///
    /// # Arguments
    /// * `command` - Validated command containing username, email, and password
    ///
    /// # Returns
    /// Created user entity
    ///
    /// # Errors
    /// * `UsernameAlreadyExists` - Username is already taken
    /// * `EmailAlreadyExists` - Email is already registered
    /// * `DatabaseError` - Database operation failed
    async fn create_user(&self, command: CreateUserCommand) -> Result<User, UserError>;

    /// Retrieve user by unique identifier.
    ///
    /// # Arguments
    /// * `id` - User ID
    ///
    /// # Returns
    /// User entity
    ///
    /// # Errors
    /// * `NotFound` - User does not exist
    /// * `DatabaseError` - Database operation failed
    async fn get_user(&self, id: &UserId) -> Result<User, UserError>;

    /// Retrieve user by unique username.
    ///
    /// # Arguments
    /// * `username` - Username to search for
    ///
    /// # Returns
    /// User entity
    ///
    /// # Errors
    /// * `NotFoundByUsername` - No user with this username
    /// * `DatabaseError` - Database operation failed
    async fn get_user_by_username(&self, username: &Username) -> Result<User, UserError>;

    /// Retrieve multiple users by identifiers.
    ///
    /// # Arguments
    /// * `user_ids` - Slice of user IDs to retrieve
    ///
    /// # Returns
    /// Vector of found users (missing IDs are skipped without error)
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn get_users_by_ids(&self, user_ids: &[UserId]) -> Result<Vec<User>, UserError>;

    /// Update existing user with optional fields.
    ///
    /// # Arguments
    /// * `id` - User ID to update
    /// * `command` - Command with optional username, email, and password fields
    ///
    /// # Returns
    /// Updated user entity
    ///
    /// # Errors
    /// * `NotFound` - User does not exist
    /// * `UsernameAlreadyExists` - New username is already taken
    /// * `EmailAlreadyExists` - New email is already registered
    /// * `DatabaseError` - Database operation failed
    async fn update_user(&self, id: &UserId, command: UpdateUserCommand)
        -> Result<User, UserError>;

    /// Delete existing user.
    ///
    /// # Arguments
    /// * `id` - User ID to delete
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * `NotFound` - User does not exist
    /// * `DatabaseError` - Database operation failed
    async fn delete_user(&self, id: &UserId) -> Result<(), UserError>;
}

/// Persistence operations for user aggregate.
#[async_trait]
pub trait UserRepository: Send + Sync + 'static {
    /// Persist new user to storage.
    ///
    /// # Arguments
    /// * `user` - User entity to create
    ///
    /// # Returns
    /// Created user entity
    ///
    /// # Errors
    /// * `UsernameAlreadyExists` - Username is already taken
    /// * `EmailAlreadyExists` - Email is already registered
    /// * `DatabaseError` - Database operation failed
    async fn create(&self, user: User) -> Result<User, UserError>;

    /// Retrieve user by identifier.
    ///
    /// # Arguments
    /// * `id` - User ID
    ///
    /// # Returns
    /// Optional user entity (None if not found)
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, UserError>;

    /// Retrieve user by username.
    ///
    /// # Arguments
    /// * `username` - Username to search for
    ///
    /// # Returns
    /// Optional user entity (None if not found)
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn find_by_username(&self, username: &Username) -> Result<Option<User>, UserError>;

    /// Retrieve user by email address.
    ///
    /// # Arguments
    /// * `email` - Email address string
    ///
    /// # Returns
    /// Optional user entity (None if not found)
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn find_by_email(&self, email: &str) -> Result<Option<User>, UserError>;

    /// Retrieve all users from storage.
    ///
    /// # Returns
    /// Vector of all users
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn list_all(&self) -> Result<Vec<User>, UserError>;

    /// Retrieve multiple users by identifiers.
    ///
    /// # Arguments
    /// * `ids` - Slice of user IDs to retrieve
    ///
    /// # Returns
    /// Vector of found users (missing IDs are skipped without error)
    ///
    /// # Errors
    /// * `DatabaseError` - Database operation failed
    async fn find_by_ids(&self, ids: &[UserId]) -> Result<Vec<User>, UserError>;

    /// Update existing user in storage.
    ///
    /// # Arguments
    /// * `user` - User entity with updated fields
    ///
    /// # Returns
    /// Updated user entity
    ///
    /// # Errors
    /// * `NotFound` - User does not exist
    /// * `UsernameAlreadyExists` - New username is already taken
    /// * `EmailAlreadyExists` - New email is already registered
    /// * `DatabaseError` - Database operation failed
    async fn update(&self, user: User) -> Result<User, UserError>;

    /// Remove user from storage.
    ///
    /// # Arguments
    /// * `id` - User ID to delete
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * `NotFound` - User does not exist
    /// * `DatabaseError` - Database operation failed
    async fn delete(&self, id: &UserId) -> Result<(), UserError>;
}

/// Event publishing for domain events.
#[async_trait]
pub trait EventPublisher: Send + Sync + 'static {
    /// Publish user creation event.
    ///
    /// # Arguments
    /// * `event` - UserCreated event
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * `SerializationFailed` - Event serialization failed
    /// * `PublishFailed` - Failed to publish to broker
    /// * `ConnectionFailed` - Broker connection failed
    /// * `Timeout` - Publishing timed out
    async fn publish_user_created(
        &self,
        event: &UserCreatedEvent,
    ) -> Result<(), EventPublisherError>;

    /// Publish user update event.
    ///
    /// # Arguments
    /// * `event` - UserUpdated event
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * `SerializationFailed` - Event serialization failed
    /// * `PublishFailed` - Failed to publish to broker
    /// * `ConnectionFailed` - Broker connection failed
    /// * `Timeout` - Publishing timed out
    async fn publish_user_updated(
        &self,
        event: &UserUpdatedEvent,
    ) -> Result<(), EventPublisherError>;

    /// Publish user deletion event.
    ///
    /// # Arguments
    /// * `event` - UserDeleted event
    ///
    /// # Returns
    /// Unit on success
    ///
    /// # Errors
    /// * `SerializationFailed` - Event serialization failed
    /// * `PublishFailed` - Failed to publish to broker
    /// * `ConnectionFailed` - Broker connection failed
    /// * `Timeout` - Publishing timed out
    async fn publish_user_deleted(
        &self,
        event: &UserDeletedEvent,
    ) -> Result<(), EventPublisherError>;
}
