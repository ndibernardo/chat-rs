use thiserror::Error;

/// Error for UserId parsing failures
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum UserIdError {
    #[error("Invalid UUID format: {0}")]
    InvalidFormat(String),
}

/// Error for Username validation failures
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum UsernameError {
    #[error("Username too short: minimum {min} characters, got {actual}")]
    TooShort { min: usize, actual: usize },

    #[error("Username too long: maximum {max} characters, got {actual}")]
    TooLong { max: usize, actual: usize },

    #[error(
        "Username contains invalid characters (only alphanumeric, underscore, and hyphen allowed)"
    )]
    InvalidCharacters,
}

/// Top-level error type for chat-service's user-domain operations: replica
/// reads/writes and remote user-service lookups.
#[derive(Debug, Error)]
pub enum UserError {
    #[error("Invalid user ID: {0}")]
    InvalidUserId(#[from] UserIdError),

    #[error("Invalid username: {0}")]
    InvalidUsername(#[from] UsernameError),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Remote user-service error: {0}")]
    RemoteError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}
