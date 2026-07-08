use chrono::DateTime;
use chrono::Utc;

/// Envelope for all user-related events from user-service
#[derive(Debug, Clone)]
pub enum UserEvent {
    UserCreated(UserCreatedEvent),
    UserUpdated(UserUpdatedEvent),
    UserDeleted(UserDeletedEvent),
}

impl UserEvent {
    pub fn event_id(&self) -> &str {
        match self {
            UserEvent::UserCreated(e) => &e.event_id,
            UserEvent::UserUpdated(e) => &e.event_id,
            UserEvent::UserDeleted(e) => &e.event_id,
        }
    }

    pub fn event_type(&self) -> &str {
        match self {
            UserEvent::UserCreated(_) => "user_created",
            UserEvent::UserUpdated(_) => "user_updated",
            UserEvent::UserDeleted(_) => "user_deleted",
        }
    }

    pub fn user_id(&self) -> &str {
        match self {
            UserEvent::UserCreated(e) => &e.user_id,
            UserEvent::UserUpdated(e) => &e.user_id,
            UserEvent::UserDeleted(e) => &e.user_id,
        }
    }
}

/// Event published when a new user is created in user-service
#[derive(Debug, Clone)]
pub struct UserCreatedEvent {
    pub event_id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

/// Event published when a user is updated in user-service
#[derive(Debug, Clone)]
pub struct UserUpdatedEvent {
    pub event_id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub updated_at: DateTime<Utc>,
}

/// Event published when a user is deleted in user-service
#[derive(Debug, Clone)]
pub struct UserDeletedEvent {
    pub event_id: String,
    pub user_id: String,
    pub deleted_at: DateTime<Utc>,
}
