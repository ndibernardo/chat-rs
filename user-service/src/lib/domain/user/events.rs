use chrono::DateTime;
use chrono::Utc;
use uuid::Uuid;

use crate::domain::user::models::User;

/// Envelope for all user-related domain events.
#[derive(Debug, Clone)]
pub enum UserEvent {
    UserCreated(UserCreatedEvent),
    UserUpdated(UserUpdatedEvent),
    UserDeleted(UserDeletedEvent),
}

impl UserEvent {
    /// Extract the unique event identifier.
    ///
    /// # Returns
    /// Event ID string slice
    pub fn event_id(&self) -> &str {
        match self {
            UserEvent::UserCreated(e) => &e.event_id,
            UserEvent::UserUpdated(e) => &e.event_id,
            UserEvent::UserDeleted(e) => &e.event_id,
        }
    }

    /// Get the event type name.
    ///
    /// # Returns
    /// Event type string ("user_created", "user_updated", or "user_deleted")
    pub fn event_type(&self) -> &str {
        match self {
            UserEvent::UserCreated(_) => "user_created",
            UserEvent::UserUpdated(_) => "user_updated",
            UserEvent::UserDeleted(_) => "user_deleted",
        }
    }

    /// Extract the user ID this event relates to.
    ///
    /// # Returns
    /// User ID string slice
    pub fn user_id(&self) -> &str {
        match self {
            UserEvent::UserCreated(e) => &e.user_id,
            UserEvent::UserUpdated(e) => &e.user_id,
            UserEvent::UserDeleted(e) => &e.user_id,
        }
    }
}

/// Domain event published when a new user is created.
///
/// Contains snapshot of user data at creation time for downstream consumers.
#[derive(Debug, Clone)]
pub struct UserCreatedEvent {
    pub event_id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

impl UserCreatedEvent {
    /// Create a new UserCreated event from a user entity.
    ///
    /// Generates a unique event ID and extracts user data for serialization.
    ///
    /// # Arguments
    /// * `user` - User entity that was created
    ///
    /// # Returns
    /// UserCreatedEvent with unique event ID and user snapshot
    pub fn new(user: &User) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            user_id: user.id.to_string(),
            username: user.username.as_str().to_string(),
            email: user.email.as_str().to_string(),
            created_at: user.created_at,
        }
    }
}

/// Domain event published when a user is updated.
///
/// Contains snapshot of updated user data for downstream consumers.
#[derive(Debug, Clone)]
pub struct UserUpdatedEvent {
    pub event_id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub updated_at: DateTime<Utc>,
}

impl UserUpdatedEvent {
    /// Create a new UserUpdated event from a user entity.
    ///
    /// Generates a unique event ID and captures current timestamp.
    ///
    /// # Arguments
    /// * `user` - User entity with updated data
    ///
    /// # Returns
    /// UserUpdatedEvent with unique event ID and user snapshot
    pub fn new(user: &User) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            user_id: user.id.to_string(),
            username: user.username.as_str().to_string(),
            email: user.email.as_str().to_string(),
            updated_at: Utc::now(),
        }
    }
}

/// Domain event published when a user is deleted.
///
/// Contains only user ID and deletion timestamp for cleanup operations.
#[derive(Debug, Clone)]
pub struct UserDeletedEvent {
    pub event_id: String,
    pub user_id: String,
    pub deleted_at: DateTime<Utc>,
}

impl UserDeletedEvent {
    /// Create a new UserDeleted event.
    ///
    /// Generates a unique event ID and captures current timestamp.
    ///
    /// # Arguments
    /// * `user_id` - ID of the deleted user
    ///
    /// # Returns
    /// UserDeletedEvent with unique event ID and deletion timestamp
    pub fn new(user_id: String) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            user_id,
            deleted_at: Utc::now(),
        }
    }
}
