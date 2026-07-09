pub mod channels;
pub mod messages;

// Re-export handlers for easy access
use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
pub use channels::create_channel;
pub use channels::get_channel;
pub use channels::list_public_channels;
use chrono::DateTime;
use chrono::Utc;
pub use messages::get_channel_messages;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::channel::errors::ChannelError;
use crate::domain::channel::models::Channel;
use crate::domain::message::errors::MessageError;
use crate::domain::message::models::Message;

/// Standardized API success response
#[derive(Debug, Clone, Serialize)]
pub struct ApiSuccess<T: Serialize> {
    #[serde(skip)]
    pub status: StatusCode,
    #[serde(flatten)]
    pub data: T,
}

impl<T: Serialize> ApiSuccess<T> {
    pub fn new(status: StatusCode, data: T) -> Self {
        Self { status, data }
    }
}

impl<T: Serialize> IntoResponse for ApiSuccess<T> {
    fn into_response(self) -> Response {
        (self.status, Json(self.data)).into_response()
    }
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

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
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
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
pub struct ChannelResponseData {
    pub id: Uuid,
    pub channel_type: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<&Channel> for ChannelResponseData {
    fn from(channel: &Channel) -> Self {
        Self {
            id: channel.id().into_uuid(),
            channel_type: channel.channel_type().as_str().to_string(),
            name: channel.name().map(|n| n.as_str().to_string()),
            description: channel.description().map(|d| d.to_string()),
            created_by: channel.created_by().into_uuid(),
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
            ChannelError::DirectChannelAlreadyExists => {
                ApiError::UnprocessableEntity(err.to_string())
            }
            ChannelError::SelfDirectChannel(_) => ApiError::UnprocessableEntity(err.to_string()),
            ChannelError::InvalidChannelId(_)
            | ChannelError::InvalidChannelName(_)
            | ChannelError::InvalidUserId(_)
            | ChannelError::InvalidChannelType(_) => ApiError::UnprocessableEntity(err.to_string()),
            ChannelError::UserServiceError(msg) => ApiError::ServiceUnavailable(msg),
            ChannelError::DatabaseError(msg) | ChannelError::Unknown(msg) => {
                ApiError::InternalServerError(msg)
            }
            ChannelError::NotMember {
                user_id,
                channel_id,
            } => ApiError::Forbidden(format!(
                "User {} is not a member of channel {}",
                user_id, channel_id
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageResponseData {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub user_id: Uuid,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl From<&Message> for MessageResponseData {
    fn from(message: &Message) -> Self {
        Self {
            id: message.id().into_uuid(),
            channel_id: message.channel_id().into_uuid(),
            user_id: message.user_id().into_uuid(),
            content: message.content().as_str().to_string(),
            timestamp: message.timestamp(),
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
