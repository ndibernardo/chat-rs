pub mod channels;
pub mod messages;

// Re-export handlers for easy access
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
pub use channels::create_channel;
pub use channels::get_channel;
pub use channels::list_public_channels;
use chrono::DateTime;
use chrono::Utc;
pub use messages::get_channel_messages;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use crate::domain::channel::errors::ChannelError;
use crate::domain::channel::models::Channel;
use crate::domain::message::errors::MessageError;
use crate::domain::message::models::Message;
use crate::inbound::http::messages::ChannelIdMessage;
use crate::inbound::http::messages::MessageIdMessage;
use crate::inbound::http::messages::UserIdMessage;

/// Standardized API success response
#[derive(Debug, Clone, Serialize)]
pub struct ApiSuccess<T: Serialize> {
    #[serde(flatten)]
    pub data: T,
}

impl<T: Serialize> ApiSuccess<T> {
    pub fn new(_status: StatusCode, data: T) -> Self {
        Self { data }
    }
}

impl<T: Serialize> IntoResponse for ApiSuccess<T> {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self.data)).into_response()
    }
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unprocessable entity: {0}")]
    UnprocessableEntity(String),

    #[error("Internal server error: {0}")]
    InternalServerError(String),

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::UnprocessableEntity(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg),
            ApiError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ApiError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
        };

        let body = Json(serde_json::json!({
            "error": message
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateChannelResponseData {
    pub id: ChannelIdMessage,
    pub channel_type: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub created_by: UserIdMessage,
    pub created_at: DateTime<Utc>,
}

impl From<&Channel> for CreateChannelResponseData {
    fn from(channel: &Channel) -> Self {
        Self {
            id: channel.id().into(),
            channel_type: match channel {
                Channel::Public(_) => "public".to_string(),
                Channel::Private(_) => "private".to_string(),
                Channel::Direct(_) => "direct".to_string(),
            },
            name: channel.name().map(|n| n.as_str().to_string()),
            description: channel.description().map(|d| d.to_string()),
            created_by: channel.created_by().into(),
            created_at: channel.created_at(),
        }
    }
}

impl From<ChannelError> for ApiError {
    fn from(err: ChannelError) -> Self {
        match err {
            ChannelError::NotFound(id) => ApiError::NotFound(format!("Channel not found: {}", id)),
            ChannelError::NameAlreadyExists(name) => {
                ApiError::UnprocessableEntity(format!("Channel name already exists: {}", name))
            }
            ChannelError::InvalidChannelId(_)
            | ChannelError::InvalidChannelName(_)
            | ChannelError::InvalidUserId(_) => ApiError::UnprocessableEntity(err.to_string()),
            ChannelError::UserServiceError(msg) => ApiError::ServiceUnavailable(msg),
            ChannelError::DatabaseError(msg) | ChannelError::Unknown(msg) => {
                ApiError::InternalServerError(msg)
            }
            ChannelError::NotMember {
                user_id,
                channel_id,
            } => ApiError::UnprocessableEntity(format!(
                "User {} is not a member of channel {}",
                user_id, channel_id
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageResponseData {
    pub id: MessageIdMessage,
    pub channel_id: ChannelIdMessage,
    pub user_id: UserIdMessage,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl From<&Message> for MessageResponseData {
    fn from(message: &Message) -> Self {
        Self {
            id: message.id.into(),
            channel_id: message.channel_id.into(),
            user_id: message.user_id.into(),
            content: message.content.as_str().to_string(),
            timestamp: message.timestamp,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "channel_type", rename_all = "snake_case")]
pub enum CreateChannelRequest {
    Public {
        name: String,
        description: Option<String>,
    },
    Private {
        name: String,
        description: Option<String>,
        members: Vec<String>, // UUID strings
    },
    Direct {
        participant_id: String, // UUID string
    },
}

/// Request DTO for sending a message
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

impl From<MessageError> for ApiError {
    fn from(err: MessageError) -> Self {
        match err {
            MessageError::NotFound(id) => ApiError::NotFound(format!("Message not found: {}", id)),
            MessageError::ChannelNotFound(id) => {
                ApiError::NotFound(format!("Channel not found: {}", id))
            }
            MessageError::UserNotFound(id) => ApiError::NotFound(format!("User not found: {}", id)),
            MessageError::InvalidMessageId(_)
            | MessageError::InvalidContent(_)
            | MessageError::InvalidChannelId(_)
            | MessageError::InvalidUserId(_) => ApiError::UnprocessableEntity(err.to_string()),
            MessageError::DatabaseError(msg) | MessageError::Unknown(msg) => {
                ApiError::InternalServerError(msg)
            }
        }
    }
}
