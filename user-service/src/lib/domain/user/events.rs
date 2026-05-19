use chrono::DateTime;
use chrono::Utc;
use uuid::Uuid;

use crate::domain::user::models::User;
use crate::domain::user::models::UserId;

/// Domain event published when a new user is created.
#[derive(Debug, Clone)]
pub struct UserCreatedEvent {
    pub event_id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

impl UserCreatedEvent {
    /// Create from a user entity; generates a unique event ID.
    pub fn new(user: &User) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            user_id: user.id().to_string(),
            username: user.username().as_str().to_string(),
            email: user.email().as_str().to_string(),
            created_at: user.created_at(),
        }
    }
}

/// Domain event published when a user is updated.
#[derive(Debug, Clone)]
pub struct UserUpdatedEvent {
    pub event_id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub updated_at: DateTime<Utc>,
}

impl UserUpdatedEvent {
    /// Create from a user entity; generates a unique event ID and captures current timestamp.
    pub fn new(user: &User) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            user_id: user.id().to_string(),
            username: user.username().as_str().to_string(),
            email: user.email().as_str().to_string(),
            updated_at: Utc::now(),
        }
    }
}

/// Domain event published when a user is deleted.
#[derive(Debug, Clone)]
pub struct UserDeletedEvent {
    pub event_id: String,
    pub user_id: String,
    pub deleted_at: DateTime<Utc>,
}

impl UserDeletedEvent {
    /// Create from a user ID; generates a unique event ID and captures current timestamp.
    pub fn new(user_id: &UserId) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            deleted_at: Utc::now(),
        }
    }
}
