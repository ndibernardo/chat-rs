use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use crate::domain::user::events::UserCreatedEvent;
use crate::domain::user::events::UserDeletedEvent;
use crate::domain::user::events::UserUpdatedEvent;

/// Serializable envelope for all user-related events.
///
/// Infrastructure representation for event publishing (Kafka, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum UserEventMessage {
    UserCreated(UserCreatedMessage),
    UserUpdated(UserUpdatedMessage),
    UserDeleted(UserDeletedMessage),
}

/// Serializable message for UserCreated domain event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCreatedMessage {
    pub event_id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

impl From<&UserCreatedEvent> for UserCreatedMessage {
    fn from(event: &UserCreatedEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            user_id: event.user_id.clone(),
            username: event.username.clone(),
            email: event.email.clone(),
            created_at: event.created_at,
        }
    }
}

impl From<UserCreatedEvent> for UserEventMessage {
    fn from(event: UserCreatedEvent) -> Self {
        UserEventMessage::UserCreated(UserCreatedMessage::from(&event))
    }
}

/// Serializable message for UserUpdated domain event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUpdatedMessage {
    pub event_id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub updated_at: DateTime<Utc>,
}

impl From<&UserUpdatedEvent> for UserUpdatedMessage {
    fn from(event: &UserUpdatedEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            user_id: event.user_id.clone(),
            username: event.username.clone(),
            email: event.email.clone(),
            updated_at: event.updated_at,
        }
    }
}

impl From<UserUpdatedEvent> for UserEventMessage {
    fn from(event: UserUpdatedEvent) -> Self {
        UserEventMessage::UserUpdated(UserUpdatedMessage::from(&event))
    }
}

/// Serializable message for UserDeleted domain event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDeletedMessage {
    pub event_id: String,
    pub user_id: String,
    pub deleted_at: DateTime<Utc>,
}

impl From<&UserDeletedEvent> for UserDeletedMessage {
    fn from(event: &UserDeletedEvent) -> Self {
        Self {
            event_id: event.event_id.clone(),
            user_id: event.user_id.clone(),
            deleted_at: event.deleted_at,
        }
    }
}

impl From<UserDeletedEvent> for UserEventMessage {
    fn from(event: UserDeletedEvent) -> Self {
        UserEventMessage::UserDeleted(UserDeletedMessage::from(&event))
    }
}
