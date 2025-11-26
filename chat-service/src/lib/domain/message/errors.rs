use thiserror::Error;

use crate::domain::channel::errors::ChannelIdError;
use crate::domain::user::errors::UserIdError;
use crate::ChannelId;
use crate::MessageId;
use crate::UserId;

/// Error type for MessageId parsing failures
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum MessageIdError {
    #[error("Invalid UUID format: {0}")]
    InvalidFormat(String),
}

/// Error type for MessageContent validation failures
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum MessageContentError {
    #[error("Message content is empty")]
    Empty,

    #[error("Message content too long: maximum {max} characters, got {actual}")]
    TooLong { max: usize, actual: usize },
}

/// Top-level error type for all message-related operations
#[derive(Debug, Error)]
pub enum MessageError {
    #[error("Invalid message ID: {0}")]
    InvalidMessageId(#[from] MessageIdError),

    #[error("Invalid message content: {0}")]
    InvalidContent(#[from] MessageContentError),

    #[error("Invalid channel ID: {0}")]
    InvalidChannelId(#[from] ChannelIdError),

    #[error("Invalid user ID: {0}")]
    InvalidUserId(#[from] UserIdError),

    // Domain-level errors
    #[error("Message not found: {0}")]
    NotFound(MessageId),

    #[error("Channel not found: {0}")]
    ChannelNotFound(ChannelId),

    #[error("User not found: {0}")]
    UserNotFound(UserId),

    // Infrastructure errors
    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<anyhow::Error> for MessageError {
    fn from(err: anyhow::Error) -> Self {
        MessageError::Unknown(err.to_string())
    }
}
