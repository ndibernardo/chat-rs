use thiserror::Error;

use crate::domain::user::errors::UserIdError;
use crate::ChannelId;
use crate::UserId;

/// Error type for ChannelId parsing failures
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ChannelIdError {
    #[error("Invalid UUID format: {0}")]
    InvalidFormat(String),
}

/// Error type for ChannelName validation failures
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ChannelNameError {
    #[error("Channel name is empty")]
    Empty,

    #[error("Channel name too long: maximum {max} characters, got {actual}")]
    TooLong { max: usize, actual: usize },
}

/// Top-level error type for all channel-related operations
#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("Invalid channel ID: {0}")]
    InvalidChannelId(#[from] ChannelIdError),

    #[error("Invalid channel name: {0}")]
    InvalidChannelName(#[from] ChannelNameError),

    #[error("Invalid user ID: {0}")]
    InvalidUserId(#[from] UserIdError),

    #[error("Channel not found: {0}")]
    NotFound(ChannelId),

    #[error("Channel name already exists: {0}")]
    NameAlreadyExists(String),

    #[error("User {user_id} is not a member of channel {channel_id}")]
    NotMember {
        user_id: UserId,
        channel_id: ChannelId,
    },

    // Infrastructure errors
    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("User service error: {0}")]
    UserServiceError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<anyhow::Error> for ChannelError {
    fn from(err: anyhow::Error) -> Self {
        ChannelError::Unknown(err.to_string())
    }
}
